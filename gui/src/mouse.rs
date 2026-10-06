use alacritty_terminal::term::TermMode;
use winit::event::MouseButton;
use winit::keyboard::ModifiersState;

/// What happened with the mouse, in cell coordinates (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    /// Moved; `held` is the button down during the move, if any.
    Move { held: Option<MouseButton> },
    WheelUp,
    WheelDown,
}


/// The report a terminal sends for `action` at `(column, line)` -- only
/// while the program asked for mouse reports (`TermMode::MOUSE_MODE`;
/// ConPTY asks for them while litastum has mouse capture on). SGR form
/// (`ESC [ < b ; x ; y M/m`) when enabled, else the legacy one.
pub fn report(action: MouseAction, column: usize, line: usize, mods: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    let (button, released) = match action {
        MouseAction::Press(button) => (button_code(button)?, false),
        MouseAction::Release(button) => (button_code(button)?, true),
        MouseAction::Move { held: Some(button) } if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) => (button_code(button)? + 32, false),
        MouseAction::Move { held: None } if mode.contains(TermMode::MOUSE_MOTION) => (3 + 32, false),
        MouseAction::Move { .. } => return None,
        MouseAction::WheelUp => (64, false),
        MouseAction::WheelDown => (65, false),
    };
    let button = button + 4 * u8::from(mods.shift_key()) + 8 * u8::from(mods.alt_key()) + 16 * u8::from(mods.control_key());

    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if released { 'm' } else { 'M' };
        return Some(format!("\x1b[<{button};{};{}{end}", column + 1, line + 1).into_bytes());
    }
    // Legacy: a release has no button of its own, and coordinates stop at 223.
    let button = if released { 3 + (button & !3) } else { button };
    let coordinate = |value: usize| (value.min(222) + 33) as u8;
    Some(vec![0x1b, b'[', b'M', button + 32, coordinate(column), coordinate(line)])
}


fn button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const SGR_CLICKS: TermMode = TermMode::MOUSE_REPORT_CLICK.union(TermMode::SGR_MOUSE);

    #[test]
    fn nothing_is_reported_without_mouse_mode() {
        assert_eq!(report(MouseAction::Press(MouseButton::Left), 0, 0, ModifiersState::empty(), TermMode::empty()), None);
    }

    #[test]
    fn sgr_press_and_release_are_one_based() {
        assert_eq!(report(MouseAction::Press(MouseButton::Left), 4, 2, ModifiersState::empty(), SGR_CLICKS).unwrap(), b"\x1b[<0;5;3M");
        assert_eq!(report(MouseAction::Release(MouseButton::Left), 4, 2, ModifiersState::empty(), SGR_CLICKS).unwrap(), b"\x1b[<0;5;3m");
    }

    #[test]
    fn drags_need_drag_mode_and_carry_the_button() {
        let drag = MouseAction::Move { held: Some(MouseButton::Left) };
        assert_eq!(report(drag, 1, 1, ModifiersState::empty(), SGR_CLICKS), None);
        assert_eq!(report(drag, 1, 1, ModifiersState::empty(), SGR_CLICKS | TermMode::MOUSE_DRAG).unwrap(), b"\x1b[<32;2;2M");
    }

    #[test]
    fn the_wheel_and_modifiers() {
        assert_eq!(report(MouseAction::WheelDown, 0, 0, ModifiersState::CONTROL, SGR_CLICKS).unwrap(), b"\x1b[<81;1;1M");
    }

    #[test]
    fn legacy_reports_offset_everything_by_32() {
        assert_eq!(report(MouseAction::Press(MouseButton::Right), 0, 0, ModifiersState::empty(), TermMode::MOUSE_REPORT_CLICK).unwrap(), [0x1b, b'[', b'M', 34, 33, 33]);
    }
}
