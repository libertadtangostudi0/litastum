use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

/// The bytes a terminal sends for a key press, xterm style; `None` for a
/// key with no sequence (a lone modifier, media keys). ConPTY turns them
/// back into console key events for litastum. `Ctrl+letter` goes by the
/// physical key, so it works on any keyboard layout.
pub fn encode(logical: &Key, physical: PhysicalKey, text: Option<&str>, mods: ModifiersState, app_cursor: bool) -> Option<Vec<u8>> {
    let modifier_param = 1 + u8::from(mods.shift_key()) + 2 * u8::from(mods.alt_key()) + 4 * u8::from(mods.control_key());
    let with_mods = modifier_param > 1;

    if let Key::Named(named) = logical {
        if let Some(bytes) = named_key(*named, mods, modifier_param, with_mods, app_cursor) {
            return Some(bytes);
        }
    }

    if mods.control_key() {
        if let Some(byte) = control_byte(physical) {
            return Some(alt_prefixed(mods, vec![byte]));
        }
    }

    let text = match (text, logical) {
        (Some(text), _) if !text.is_empty() && !text.chars().any(char::is_control) => text.to_string(),
        (_, Key::Character(text)) => text.to_string(),
        _ => return None,
    };
    Some(alt_prefixed(mods, text.into_bytes()))
}


fn named_key(named: NamedKey, mods: ModifiersState, modifier_param: u8, with_mods: bool, app_cursor: bool) -> Option<Vec<u8>> {
    let csi_letter = |letter: char| -> Vec<u8> {
        if with_mods {
            format!("\x1b[1;{modifier_param}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |number: u8| -> Vec<u8> {
        if with_mods {
            format!("\x1b[{number};{modifier_param}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let ss3_function = |letter: char| -> Vec<u8> {
        if with_mods {
            format!("\x1b[1;{modifier_param}{letter}").into_bytes()
        } else {
            format!("\x1bO{letter}").into_bytes()
        }
    };

    let bytes = match named {
        NamedKey::Enter => alt_prefixed(mods, b"\r".to_vec()),
        NamedKey::Tab if mods.shift_key() => b"\x1b[Z".to_vec(),
        NamedKey::Tab => alt_prefixed(mods, b"\t".to_vec()),
        NamedKey::Backspace if mods.control_key() => alt_prefixed(mods, vec![0x08]),
        NamedKey::Backspace => alt_prefixed(mods, vec![0x7f]),
        NamedKey::Escape => alt_prefixed(mods, vec![0x1b]),
        NamedKey::Space if mods.control_key() => vec![0x00],
        NamedKey::Space => alt_prefixed(mods, b" ".to_vec()),
        NamedKey::ArrowUp => csi_letter('A'),
        NamedKey::ArrowDown => csi_letter('B'),
        NamedKey::ArrowRight => csi_letter('C'),
        NamedKey::ArrowLeft => csi_letter('D'),
        NamedKey::Home => csi_letter('H'),
        NamedKey::End => csi_letter('F'),
        NamedKey::Insert => tilde(2),
        NamedKey::Delete => tilde(3),
        NamedKey::PageUp => tilde(5),
        NamedKey::PageDown => tilde(6),
        NamedKey::F1 => ss3_function('P'),
        NamedKey::F2 => ss3_function('Q'),
        NamedKey::F3 => ss3_function('R'),
        NamedKey::F4 => ss3_function('S'),
        NamedKey::F5 => tilde(15),
        NamedKey::F6 => tilde(17),
        NamedKey::F7 => tilde(18),
        NamedKey::F8 => tilde(19),
        NamedKey::F9 => tilde(20),
        NamedKey::F10 => tilde(21),
        NamedKey::F11 => tilde(23),
        NamedKey::F12 => tilde(24),
        _ => return None,
    };
    Some(bytes)
}


/// `Ctrl+A`..`Ctrl+Z` and the few punctuation controls, by physical key.
fn control_byte(physical: PhysicalKey) -> Option<u8> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    let letters = [
        KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG,
        KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN,
        KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU,
        KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
    ];
    if let Some(index) = letters.iter().position(|letter| *letter == code) {
        return Some(index as u8 + 1);
    }
    match code {
        KeyCode::BracketLeft => Some(0x1b),
        KeyCode::Backslash => Some(0x1c),
        KeyCode::BracketRight => Some(0x1d),
        _ => None,
    }
}


fn alt_prefixed(mods: ModifiersState, mut bytes: Vec<u8>) -> Vec<u8> {
    if mods.alt_key() {
        bytes.insert(0, 0x1b);
    }
    bytes
}


#[cfg(test)]
mod tests {
    use winit::keyboard::SmolStr;

    use super::*;

    fn named(key: NamedKey, mods: ModifiersState) -> Vec<u8> {
        encode(&Key::Named(key), PhysicalKey::Code(KeyCode::Fn), None, mods, false).unwrap()
    }

    #[test]
    fn plain_and_modified_arrows() {
        assert_eq!(named(NamedKey::ArrowLeft, ModifiersState::empty()), b"\x1b[D");
        assert_eq!(named(NamedKey::ArrowLeft, ModifiersState::CONTROL | ModifiersState::SHIFT), b"\x1b[1;6D");
        assert_eq!(encode(&Key::Named(NamedKey::ArrowUp), PhysicalKey::Code(KeyCode::ArrowUp), None, ModifiersState::empty(), true).unwrap(), b"\x1bOA");
    }

    #[test]
    fn function_keys_with_and_without_modifiers() {
        assert_eq!(named(NamedKey::F1, ModifiersState::empty()), b"\x1bOP");
        assert_eq!(named(NamedKey::F5, ModifiersState::ALT), b"\x1b[15;3~");
        assert_eq!(named(NamedKey::F7, ModifiersState::SHIFT), b"\x1b[18;2~");
        assert_eq!(named(NamedKey::F10, ModifiersState::empty()), b"\x1b[21~");
    }

    #[test]
    fn ctrl_letters_go_by_the_physical_key_on_any_layout() {
        let cyrillic = Key::Character(SmolStr::new("\u{0441}"));
        assert_eq!(encode(&cyrillic, PhysicalKey::Code(KeyCode::KeyC), None, ModifiersState::CONTROL, false).unwrap(), [0x03]);
    }

    #[test]
    fn typed_text_is_sent_as_utf8() {
        let key = Key::Character(SmolStr::new("\u{0436}"));
        assert_eq!(encode(&key, PhysicalKey::Code(KeyCode::Semicolon), Some("\u{0436}"), ModifiersState::empty(), false).unwrap(), "\u{0436}".as_bytes());
    }

    #[test]
    fn shift_tab_and_enter() {
        assert_eq!(named(NamedKey::Tab, ModifiersState::SHIFT), b"\x1b[Z");
        assert_eq!(named(NamedKey::Enter, ModifiersState::empty()), b"\r");
    }
}
