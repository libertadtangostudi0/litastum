use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

/// `win32-input-mode`: one console key event per key press *and*
/// release, as `ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc _` -- virtual key, scan
/// code, UTF-16 character, key down, control-key state, repeat count.
/// ConPTY turns it into exactly the `KEY_EVENT` the app would get in a
/// real console, so every combination survives (xterm sequences can't
/// tell `Ctrl+Shift+Z` from `Ctrl+Z`). Windows Terminal speaks the same.
pub struct KeyRecord {
    pub virtual_key: u16,
    pub scan_code: u16,
    pub unicode: u16,
    pub pressed: bool,
    pub control_state: u32,
}

impl KeyRecord {
    pub fn sequence(&self) -> Vec<u8> {
        format!("\x1b[{};{};{};{};{};1_", self.virtual_key, self.scan_code, self.unicode, u8::from(self.pressed), self.control_state).into_bytes()
    }
}

// `KEY_EVENT_RECORD::dwControlKeyState` flags.
const RIGHT_ALT_PRESSED: u32 = 0x0001;
const LEFT_ALT_PRESSED: u32 = 0x0002;
const LEFT_CTRL_PRESSED: u32 = 0x0008;
const SHIFT_PRESSED: u32 = 0x0010;
const ENHANCED_KEY: u32 = 0x0100;


/// The key record for a winit key event; `None` for a key with no
/// virtual key code here. `scan_code` comes from the platform (0 if
/// unknown -- ConPTY doesn't need it). AltGr arrives as Ctrl+Alt, as in a
/// real console.
pub fn key_record(logical: &Key, physical: PhysicalKey, text: Option<&str>, mods: ModifiersState, pressed: bool, scan_code: u16) -> Option<KeyRecord> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    let virtual_key = virtual_key(code)?;
    let mut control_state = 0;
    if mods.shift_key() {
        control_state |= SHIFT_PRESSED;
    }
    if mods.control_key() {
        control_state |= LEFT_CTRL_PRESSED;
    }
    if mods.alt_key() {
        control_state |= if code == KeyCode::AltRight { RIGHT_ALT_PRESSED } else { LEFT_ALT_PRESSED };
    }
    if is_enhanced(code) {
        control_state |= ENHANCED_KEY;
    }
    Some(KeyRecord { virtual_key, scan_code, unicode: unicode(logical, code, text, mods), pressed, control_state })
}


/// The character a console reports with the key: the typed text, the
/// control code for `Ctrl+letter`, the classic codes for Enter, Tab,
/// Backspace, Escape. 0 for keys that type nothing.
fn unicode(logical: &Key, code: KeyCode, text: Option<&str>, mods: ModifiersState) -> u16 {
    if mods.control_key() && !mods.alt_key() {
        if let Some(letter) = letter_index(code) {
            return letter + 1;
        }
    }
    match logical {
        Key::Named(NamedKey::Enter) => 13,
        Key::Named(NamedKey::Tab) => 9,
        Key::Named(NamedKey::Backspace) => 8,
        Key::Named(NamedKey::Escape) => 27,
        Key::Named(NamedKey::Space) => 32,
        _ => {
            let typed = text.filter(|text| !text.is_empty()).or(match logical {
                Key::Character(text) => Some(text.as_str()),
                _ => None,
            });
            typed.and_then(|text| text.encode_utf16().next()).unwrap_or(0)
        }
    }
}


fn letter_index(code: KeyCode) -> Option<u16> {
    LETTERS.iter().position(|letter| *letter == code).map(|index| index as u16)
}


const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG,
    KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN,
    KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU,
    KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4,
    KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
];

const FUNCTION_KEYS: [KeyCode; 12] = [
    KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6,
    KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
];

const NUMPAD_DIGITS: [KeyCode; 10] = [
    KeyCode::Numpad0, KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3, KeyCode::Numpad4,
    KeyCode::Numpad5, KeyCode::Numpad6, KeyCode::Numpad7, KeyCode::Numpad8, KeyCode::Numpad9,
];


/// Windows virtual-key codes (`VK_*`) by physical key.
fn virtual_key(code: KeyCode) -> Option<u16> {
    if let Some(index) = letter_index(code) {
        return Some(0x41 + index);
    }
    let in_table = |table: &[KeyCode], base: u16| table.iter().position(|key| *key == code).map(|index| base + index as u16);
    if let Some(vk) = in_table(&DIGITS, 0x30).or_else(|| in_table(&FUNCTION_KEYS, 0x70)).or_else(|| in_table(&NUMPAD_DIGITS, 0x60)) {
        return Some(vk);
    }
    Some(match code {
        KeyCode::Backspace => 0x08,
        KeyCode::Tab => 0x09,
        KeyCode::Enter | KeyCode::NumpadEnter => 0x0D,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => 0x10,
        KeyCode::ControlLeft | KeyCode::ControlRight => 0x11,
        KeyCode::AltLeft | KeyCode::AltRight => 0x12,
        KeyCode::Pause => 0x13,
        KeyCode::CapsLock => 0x14,
        KeyCode::Escape => 0x1B,
        KeyCode::Space => 0x20,
        KeyCode::PageUp => 0x21,
        KeyCode::PageDown => 0x22,
        KeyCode::End => 0x23,
        KeyCode::Home => 0x24,
        KeyCode::ArrowLeft => 0x25,
        KeyCode::ArrowUp => 0x26,
        KeyCode::ArrowRight => 0x27,
        KeyCode::ArrowDown => 0x28,
        KeyCode::PrintScreen => 0x2C,
        KeyCode::Insert => 0x2D,
        KeyCode::Delete => 0x2E,
        KeyCode::SuperLeft => 0x5B,
        KeyCode::SuperRight => 0x5C,
        KeyCode::ContextMenu => 0x5D,
        KeyCode::NumpadMultiply => 0x6A,
        KeyCode::NumpadAdd => 0x6B,
        KeyCode::NumpadSubtract => 0x6D,
        KeyCode::NumpadDecimal => 0x6E,
        KeyCode::NumpadDivide => 0x6F,
        KeyCode::NumLock => 0x90,
        KeyCode::ScrollLock => 0x91,
        KeyCode::Semicolon => 0xBA,
        KeyCode::Equal => 0xBB,
        KeyCode::Comma => 0xBC,
        KeyCode::Minus => 0xBD,
        KeyCode::Period => 0xBE,
        KeyCode::Slash => 0xBF,
        KeyCode::Backquote => 0xC0,
        KeyCode::BracketLeft => 0xDB,
        KeyCode::Backslash => 0xDC,
        KeyCode::BracketRight => 0xDD,
        KeyCode::Quote => 0xDE,
        KeyCode::IntlBackslash => 0xE2,
        _ => return None,
    })
}


/// The keys a console flags `ENHANCED_KEY`: the separate navigation block,
/// the right-hand modifiers, numpad Enter and Divide.
fn is_enhanced(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Insert
            | KeyCode::Delete
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::ArrowLeft
            | KeyCode::ArrowUp
            | KeyCode::ArrowRight
            | KeyCode::ArrowDown
            | KeyCode::ControlRight
            | KeyCode::AltRight
            | KeyCode::NumpadEnter
            | KeyCode::NumpadDivide
    )
}


#[cfg(test)]
mod tests {
    use winit::keyboard::SmolStr;

    use super::*;

    fn record(logical: Key, code: KeyCode, text: Option<&str>, mods: ModifiersState) -> KeyRecord {
        key_record(&logical, PhysicalKey::Code(code), text, mods, true, 0).unwrap()
    }

    #[test]
    fn a_typed_letter_carries_its_character() {
        let key = record(Key::Character(SmolStr::new("a")), KeyCode::KeyA, Some("a"), ModifiersState::empty());
        assert_eq!(key.sequence(), b"\x1b[65;0;97;1;0;1_");
    }

    /// The case xterm sequences lose: Ctrl+Shift+Z is its own event.
    #[test]
    fn ctrl_shift_z_keeps_both_modifiers() {
        let key = record(Key::Character(SmolStr::new("Z")), KeyCode::KeyZ, None, ModifiersState::CONTROL | ModifiersState::SHIFT);
        assert_eq!((key.virtual_key, key.unicode, key.control_state), (0x5A, 26, LEFT_CTRL_PRESSED | SHIFT_PRESSED));
    }

    #[test]
    fn ctrl_letters_go_by_the_physical_key_on_any_layout() {
        let key = record(Key::Character(SmolStr::new("\u{0441}")), KeyCode::KeyC, None, ModifiersState::CONTROL);
        assert_eq!((key.virtual_key, key.unicode), (0x43, 3));
    }

    #[test]
    fn cyrillic_text_is_sent_as_utf16() {
        let key = record(Key::Character(SmolStr::new("\u{0436}")), KeyCode::Semicolon, Some("\u{0436}"), ModifiersState::empty());
        assert_eq!((key.virtual_key, key.unicode), (0xBA, 0x0436));
    }

    #[test]
    fn navigation_keys_are_enhanced() {
        let key = record(Key::Named(NamedKey::ArrowLeft), KeyCode::ArrowLeft, None, ModifiersState::CONTROL | ModifiersState::SHIFT);
        assert_eq!(key.sequence(), format!("\x1b[37;0;0;1;{};1_", LEFT_CTRL_PRESSED | SHIFT_PRESSED | ENHANCED_KEY).into_bytes());
    }

    #[test]
    fn function_keys_and_releases() {
        let key = key_record(&Key::Named(NamedKey::F5), PhysicalKey::Code(KeyCode::F5), None, ModifiersState::ALT, false, 0x3F).unwrap();
        assert_eq!(key.sequence(), format!("\x1b[116;63;0;0;{LEFT_ALT_PRESSED};1_").into_bytes());
    }

    #[test]
    fn enter_and_escape_report_their_classic_codes() {
        assert_eq!(record(Key::Named(NamedKey::Enter), KeyCode::Enter, Some("\r"), ModifiersState::empty()).unicode, 13);
        assert_eq!(record(Key::Named(NamedKey::Escape), KeyCode::Escape, None, ModifiersState::empty()).unicode, 27);
    }
}
