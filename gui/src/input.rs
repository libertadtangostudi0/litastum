use alacritty_terminal::term::TermMode;
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

/// `Ctrl+V` (by physical key, any layout) or `Shift+Insert`: pasted by
/// the window itself, as Windows Terminal does.
pub fn is_paste_chord(logical: &Key, physical: PhysicalKey, mods: ModifiersState) -> bool {
    let ctrl_v = mods.control_key() && !mods.shift_key() && !mods.alt_key() && physical == PhysicalKey::Code(KeyCode::KeyV);
    let shift_insert = mods.shift_key() && !mods.control_key() && *logical == Key::Named(NamedKey::Insert);
    ctrl_v || shift_insert
}


/// A font size change asked for with the keyboard or `Ctrl`+wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

/// Smallest and largest font size zoom allows, in logical pixels.
pub const MIN_FONT_SIZE: f32 = 6.0;
pub const MAX_FONT_SIZE: f32 = 72.0;
/// One zoom step.
const ZOOM_STEP: f32 = 1.0;


/// `Ctrl+=` (`Ctrl++`), `Ctrl+-` and `Ctrl+0`, on the main row or the
/// numpad, by physical key so any layout works -- handled by the window,
/// as Windows Terminal does, never sent to the program.
pub fn zoom_chord(physical: PhysicalKey, mods: ModifiersState) -> Option<Zoom> {
    if !mods.control_key() || mods.alt_key() {
        return None;
    }
    match physical {
        PhysicalKey::Code(KeyCode::Equal | KeyCode::NumpadAdd) => Some(Zoom::In),
        PhysicalKey::Code(KeyCode::Minus | KeyCode::NumpadSubtract) => Some(Zoom::Out),
        PhysicalKey::Code(KeyCode::Digit0 | KeyCode::Numpad0) if !mods.shift_key() => Some(Zoom::Reset),
        _ => None,
    }
}


/// The font size after `zoom`, from `current`, within the allowed range.
pub fn zoomed(current: f32, zoom: Zoom, default: f32) -> f32 {
    let size = match zoom {
        Zoom::In => current + ZOOM_STEP,
        Zoom::Out => current - ZOOM_STEP,
        Zoom::Reset => default,
    };
    size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}


/// The clipboard's text as the program's input: line breaks as Enter,
/// bracketed when the program asked for it (`bracketed`).
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}


/// Whether typed `text` can't travel as one console key record: a
/// character outside the Basic Multilingual Plane (most emoji) is two
/// UTF-16 units, and a record carries one. Sent as plain UTF-8 instead,
/// which ConPTY splits into records itself.
pub fn needs_plain_text(text: Option<&str>) -> bool {
    text.is_some_and(|text| text.chars().any(|c| c.len_utf16() > 1) || text.chars().count() > 1)
}


/// `CSI I`/`CSI O` on focus change, while the program asked for them
/// (`?1004h` -- ConPTY does at startup).
pub fn focus_report(focused: bool, mode: TermMode) -> Option<&'static [u8]> {
    mode.contains(TermMode::FOCUS_IN_OUT).then_some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}


/// The wheel without mouse reports, on the alternate screen: arrow keys,
/// one per line, as Windows Terminal and xterm's alternate scroll mode
/// do -- the panels move their selection. `None` when the program takes
/// mouse reports (`mouse::report`) or isn't on the alternate screen.
pub fn alternate_scroll(lines: i32, mode: TermMode) -> Option<Vec<u8>> {
    if lines == 0 || mode.intersects(TermMode::MOUSE_MODE) || !mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        return None;
    }
    let arrow: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
        (true, false) => b"\x1b[A",
        (true, true) => b"\x1bOA",
        (false, false) => b"\x1b[B",
        (false, true) => b"\x1bOB",
    };
    Some(arrow.repeat(lines.unsigned_abs() as usize))
}


/// Wheel and touchpad movement in whole lines (positive up). Touchpads
/// and fine-grained wheels report fractions of a line, which used to
/// round to nothing on every event; the remainder now carries over.
#[derive(Default)]
pub struct WheelAccumulator {
    remainder: f64,
}

impl WheelAccumulator {
    pub fn lines(&mut self, delta: MouseScrollDelta, cell_height: u32) -> i32 {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, lines) => f64::from(lines),
            MouseScrollDelta::PixelDelta(position) => position.y / f64::from(cell_height.max(1)),
        };
        self.remainder += lines;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        whole as i32
    }
}


#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalPosition;

    use super::*;

    #[test]
    fn touchpad_fractions_add_up_to_whole_lines() {
        let mut wheel = WheelAccumulator::default();
        let pixels = |y: f64| MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y));
        assert_eq!(wheel.lines(pixels(8.0), 20), 0, "0.4 of a line");
        assert_eq!(wheel.lines(pixels(8.0), 20), 0, "0.8");
        assert_eq!(wheel.lines(pixels(8.0), 20), 1, "1.2: one line, 0.2 left");
        assert_eq!(wheel.lines(MouseScrollDelta::LineDelta(0.0, -1.5), 20), -1, "-1.3");
    }

    #[test]
    fn a_whole_wheel_notch_is_one_line_at_once() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.lines(MouseScrollDelta::LineDelta(0.0, 3.0), 20), 3);
    }

    #[test]
    fn the_wheel_becomes_arrows_on_the_alternate_screen_without_mouse_reports() {
        let screen = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(alternate_scroll(2, screen).unwrap(), b"\x1b[A\x1b[A");
        assert_eq!(alternate_scroll(-1, screen | TermMode::APP_CURSOR).unwrap(), b"\x1bOB");
        assert_eq!(alternate_scroll(1, screen | TermMode::MOUSE_REPORT_CLICK), None, "the program takes the mouse");
        assert_eq!(alternate_scroll(1, TermMode::ALTERNATE_SCROLL), None, "not on the alternate screen");
    }

    #[test]
    fn focus_is_reported_only_when_asked_for() {
        assert_eq!(focus_report(true, TermMode::FOCUS_IN_OUT), Some(&b"\x1b[I"[..]));
        assert_eq!(focus_report(false, TermMode::FOCUS_IN_OUT), Some(&b"\x1b[O"[..]));
        assert_eq!(focus_report(true, TermMode::empty()), None);
    }

    #[test]
    fn text_outside_the_bmp_goes_as_plain_text() {
        assert!(needs_plain_text(Some("\u{1F600}")));
        assert!(!needs_plain_text(Some("\u{0436}")));
        assert!(!needs_plain_text(None));
    }

    #[test]
    fn pasted_line_breaks_become_enter() {
        assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc");
        assert_eq!(paste_bytes("x", true), b"\x1b[200~x\x1b[201~");
    }

    #[test]
    fn zoom_chords_by_physical_key() {
        let ctrl = ModifiersState::CONTROL;
        assert_eq!(zoom_chord(PhysicalKey::Code(KeyCode::Equal), ctrl), Some(Zoom::In));
        assert_eq!(zoom_chord(PhysicalKey::Code(KeyCode::Equal), ctrl | ModifiersState::SHIFT), Some(Zoom::In), "Ctrl++");
        assert_eq!(zoom_chord(PhysicalKey::Code(KeyCode::NumpadSubtract), ctrl), Some(Zoom::Out));
        assert_eq!(zoom_chord(PhysicalKey::Code(KeyCode::Digit0), ctrl), Some(Zoom::Reset));
        assert_eq!(zoom_chord(PhysicalKey::Code(KeyCode::Minus), ModifiersState::empty()), None, "a plain minus is typed");
        assert_eq!(zoom_chord(PhysicalKey::Code(KeyCode::Minus), ctrl | ModifiersState::ALT), None);
    }

    #[test]
    fn zoom_steps_and_stays_in_range() {
        assert_eq!(zoomed(15.0, Zoom::In, 15.0), 16.0);
        assert_eq!(zoomed(15.0, Zoom::Out, 15.0), 14.0);
        assert_eq!(zoomed(30.0, Zoom::Reset, 15.0), 15.0);
        assert_eq!(zoomed(MAX_FONT_SIZE, Zoom::In, 15.0), MAX_FONT_SIZE);
        assert_eq!(zoomed(MIN_FONT_SIZE, Zoom::Out, 15.0), MIN_FONT_SIZE);
    }

    #[test]
    fn paste_chords() {
        let v = PhysicalKey::Code(KeyCode::KeyV);
        assert!(is_paste_chord(&Key::Character("\u{043c}".into()), v, ModifiersState::CONTROL), "Ctrl+V on the Russian layout");
        assert!(!is_paste_chord(&Key::Character("v".into()), v, ModifiersState::CONTROL | ModifiersState::SHIFT));
        assert!(is_paste_chord(&Key::Named(NamedKey::Insert), PhysicalKey::Code(KeyCode::Insert), ModifiersState::SHIFT));
    }
}
