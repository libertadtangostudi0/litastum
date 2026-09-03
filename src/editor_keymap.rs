use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};


/// A user-triggered action while a file is open in the built-in editor.
/// Kept separate from the browsing-mode `keymap::Command` because the
/// editor's default action is "forward everything to the text widget"
/// rather than "look up a binding or do nothing" — see `Input` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    Close,
    Save,
    Copy,
    Cut,
    Paste,
    /// Not one of the bindings above — forward the raw key event to
    /// the text area as ordinary typing/movement.
    Input,
}


/// Resolves a raw key press to an `EditorCommand`. Unlike
/// `keymap::resolve`, this never returns `None` — an unrecognized key
/// still needs to reach the text area.
///
/// Matches both the lowercase and uppercase letter for each `Ctrl`
/// binding: some terminal/backend combinations report the
/// Caps-Lock-affected case even while `Ctrl` is held, so `Ctrl+S` can
/// arrive as `Char('S')` rather than `Char('s')` — this was found by
/// hand while debugging save/paste appearing to silently do nothing.
pub fn resolve(key: KeyEvent) -> EditorCommand {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Esc => EditorCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => EditorCommand::Save,
        KeyCode::Char('c' | 'C') if ctrl => EditorCommand::Copy,
        KeyCode::Char('x' | 'X') if ctrl => EditorCommand::Cut,
        KeyCode::Char('v' | 'V') if ctrl => EditorCommand::Paste,
        _ => EditorCommand::Input,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_s_resolves_to_save() {
        assert_eq!(resolve(ctrl_key('s')), EditorCommand::Save);
    }

    #[test]
    fn ctrl_shift_s_uppercase_still_resolves_to_save() {
        assert_eq!(resolve(ctrl_key('S')), EditorCommand::Save);
    }

    #[test]
    fn ctrl_c_resolves_to_copy() {
        assert_eq!(resolve(ctrl_key('c')), EditorCommand::Copy);
    }

    #[test]
    fn ctrl_x_resolves_to_cut() {
        assert_eq!(resolve(ctrl_key('x')), EditorCommand::Cut);
    }

    #[test]
    fn ctrl_v_resolves_to_paste() {
        assert_eq!(resolve(ctrl_key('v')), EditorCommand::Paste);
    }

    #[test]
    fn esc_resolves_to_close_even_without_ctrl() {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Close);
    }

    #[test]
    fn plain_s_without_ctrl_is_ordinary_input_not_save() {
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Input);
    }

    #[test]
    fn unmodified_letter_is_input() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Input);
    }
}
