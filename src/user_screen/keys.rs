use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The bytes a terminal sends for `key`, xterm style, for the program
/// running in the pseudoconsole (ConPTY turns them back into console key
/// events). `None` for a key with no sequence. `app_cursor`: the program
/// asked for application cursor keys. As `gui/src/keys.rs`, for
/// `crossterm`'s keys.
pub fn encode_key(key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let parameter = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let with_modifiers = parameter > 1;

    let csi_letter = |letter: char| -> Vec<u8> {
        if with_modifiers {
            format!("\x1b[1;{parameter}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |number: u8| -> Vec<u8> {
        if with_modifiers {
            format!("\x1b[{number};{parameter}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let ss3 = |letter: char| -> Vec<u8> {
        if with_modifiers {
            format!("\x1b[1;{parameter}{letter}").into_bytes()
        } else {
            format!("\x1bO{letter}").into_bytes()
        }
    };
    let alt_prefixed = |mut bytes: Vec<u8>| -> Vec<u8> {
        if alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };

    let bytes = match key.code {
        KeyCode::Enter => alt_prefixed(b"\r".to_vec()),
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Tab => alt_prefixed(b"\t".to_vec()),
        KeyCode::Backspace if ctrl => alt_prefixed(vec![0x08]),
        KeyCode::Backspace => alt_prefixed(vec![0x7f]),
        KeyCode::Esc => alt_prefixed(vec![0x1b]),
        KeyCode::Up => csi_letter('A'),
        KeyCode::Down => csi_letter('B'),
        KeyCode::Right => csi_letter('C'),
        KeyCode::Left => csi_letter('D'),
        KeyCode::Home => csi_letter('H'),
        KeyCode::End => csi_letter('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) => ss3((b'P' + n - 1) as char),
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        KeyCode::Char(' ') if ctrl => vec![0x00],
        KeyCode::Char(c) if ctrl => alt_prefixed(vec![control_byte(c)?]),
        KeyCode::Char(c) => alt_prefixed(c.to_string().into_bytes()),
        _ => return None,
    };
    Some(bytes)
}


/// `Ctrl+A`..`Ctrl+Z` and the few punctuation controls. A letter typed
/// under another layout is already Latin here
/// (`keyboard_layout::normalize_ctrl_shortcut`).
fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        letter @ 'a'..='z' => Some(letter as u8 - b'a' + 1),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        _ => None,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Option<Vec<u8>> {
        encode_key(KeyEvent::new(code, modifiers), false)
    }

    #[test]
    fn text_and_the_editing_keys() {
        assert_eq!(press(KeyCode::Char('ж'), KeyModifiers::NONE).unwrap(), "ж".as_bytes());
        assert_eq!(press(KeyCode::Enter, KeyModifiers::NONE).unwrap(), b"\r");
        assert_eq!(press(KeyCode::Backspace, KeyModifiers::NONE).unwrap(), b"\x7f");
        assert_eq!(press(KeyCode::Tab, KeyModifiers::SHIFT).unwrap(), b"\x1b[Z");
    }

    /// `Ctrl+C` must reach the program as the interrupt byte.
    #[test]
    fn ctrl_letters_are_control_bytes() {
        assert_eq!(press(KeyCode::Char('c'), KeyModifiers::CONTROL).unwrap(), [0x03]);
        assert_eq!(press(KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT).unwrap(), [0x1a]);
        assert_eq!(press(KeyCode::Char('x'), KeyModifiers::ALT).unwrap(), b"\x1bx");
    }

    #[test]
    fn arrows_follow_the_cursor_mode_and_carry_modifiers() {
        assert_eq!(press(KeyCode::Up, KeyModifiers::NONE).unwrap(), b"\x1b[A");
        assert_eq!(encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true).unwrap(), b"\x1bOA");
        assert_eq!(press(KeyCode::Right, KeyModifiers::CONTROL).unwrap(), b"\x1b[1;5C");
    }

    #[test]
    fn function_keys() {
        assert_eq!(press(KeyCode::F(1), KeyModifiers::NONE).unwrap(), b"\x1bOP");
        assert_eq!(press(KeyCode::F(5), KeyModifiers::NONE).unwrap(), b"\x1b[15~");
        assert_eq!(press(KeyCode::F(12), KeyModifiers::SHIFT).unwrap(), b"\x1b[24;2~");
        assert_eq!(press(KeyCode::Delete, KeyModifiers::NONE).unwrap(), b"\x1b[3~");
    }
}
