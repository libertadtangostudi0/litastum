use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};


/// A user-triggered action while a file is open in the built-in editor,
/// at the level `main.rs` needs to care about. Everything else —
/// typing, movement, selection, copy/cut/paste — is `edtui`'s own
/// concern once a key reaches `Editor::input`; only `Save` (a concept
/// `edtui` has no notion of) and `Close` (which `main.rs` must decide
/// whether to honor immediately or forward, depending on whether a
/// selection is active — see `Editor::has_selection`) need resolving
/// before that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    Close,
    Save,
    /// Not one of the bindings above — forward the raw key event to
    /// `Editor::input`.
    Forward,
}


/// Resolves a raw key press to an `EditorCommand`.
///
/// Matches both the lowercase and uppercase letter for `Ctrl+S`: some
/// terminal/backend combinations report the Caps-Lock-affected case
/// even while `Ctrl` is held, so it can arrive as `Char('S')` rather
/// than `Char('s')` — this was found by hand while debugging save
/// appearing to silently do nothing (back when this also handled
/// copy/paste directly, before the `edtui` switch).
pub fn resolve(key: KeyEvent) -> EditorCommand {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Esc => EditorCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => EditorCommand::Save,
        _ => EditorCommand::Forward,
    }
}


/// The choice on the "discard unsaved changes?" prompt (`Mode::ConfirmDiscard`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDiscardCommand {
    Discard,
    Cancel,
    /// Anything else — the prompt only understands these two answers,
    /// so unrecognized keys are ignored rather than forwarded anywhere
    /// (there's no text area to forward them to while it's showing).
    Ignore,
}


/// Resolves a raw key press on the discard-confirmation prompt.
pub fn resolve_confirm_discard(key: KeyEvent) -> ConfirmDiscardCommand {
    match key.code {
        KeyCode::Char('y' | 'Y') => ConfirmDiscardCommand::Discard,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => ConfirmDiscardCommand::Cancel,
        _ => ConfirmDiscardCommand::Ignore,
    }
}


/// Key handling while a file is open in the built-in editor. `Ctrl+S`
/// and `Esc` are the only things this module resolves itself (`resolve`
/// above) — everything else, including copy/cut/paste/selection, is
/// `edtui`'s own concern once forwarded to `Editor::input`. `Esc` is
/// special-cased further: with an active selection it's forwarded too
/// (so `edtui`'s own binding cancels the selection), only closing the
/// editor once there's nothing selected.
///
/// Moved here from `main.rs` alongside `resolve`/`resolve_confirm_discard`
/// so this module owns editor key handling end to end, the same way
/// `theme_menu.rs`/`menu.rs` each own their own state and handling —
/// `main.rs` stays a thin dispatcher over `Mode`.
pub fn handle_editor_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let command = resolve(key);
    debug!(?key, ?command, "editor key");

    if command == EditorCommand::Close {
        let has_selection = matches!(&app.mode, Mode::Editing(editor) if editor.has_selection());
        if has_selection {
            let Mode::Editing(active_editor) = &mut app.mode else {
                return Ok(());
            };
            active_editor.input(key);
            return Ok(());
        }
        return close_editor_or_confirm(app);
    }

    let Mode::Editing(active_editor) = &mut app.mode else {
        return Ok(());
    };

    match command {
        EditorCommand::Close => unreachable!("handled above"),
        EditorCommand::Save => active_editor.save()?,
        EditorCommand::Forward => active_editor.input(key),
    }

    Ok(())
}


/// `Esc` in the editor: closes straight back to browsing if the buffer
/// has no unsaved changes, otherwise moves to `Mode::ConfirmDiscard`
/// instead of discarding them silently.
fn close_editor_or_confirm(app: &mut App) -> Result<()> {
    let Mode::Editing(editor) = &app.mode else {
        return Ok(());
    };

    if !editor.is_dirty() {
        app.mode = Mode::Browsing;
        app.active_panel().reload()?;
        return Ok(());
    }

    debug!("editor close: unsaved changes, asking to confirm discard");
    let Mode::Editing(editor) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::Editing above");
    };
    app.mode = Mode::ConfirmDiscard(editor);
    Ok(())
}


/// Key handling on the "discard unsaved changes?" prompt: `Y` discards
/// and returns to browsing, `N`/`Esc` cancels back into the editor with
/// nothing lost, anything else is ignored.
pub fn handle_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let command = resolve_confirm_discard(key);
    debug!(?key, ?command, "confirm-discard key");

    match command {
        ConfirmDiscardCommand::Discard => {
            app.mode = Mode::Browsing;
            app.active_panel().reload()?;
        }
        ConfirmDiscardCommand::Cancel => {
            let Mode::ConfirmDiscard(editor) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::ConfirmDiscard");
            };
            app.mode = Mode::Editing(editor);
        }
        ConfirmDiscardCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::editor::Editor;
    use crate::test_support::{ctrl_key, key, shift_key, test_app, unique_scratch_dir};

    /// A real `App` (no terminal needed) in `Mode::Editing`, with the
    /// panel rooted in the same scratch directory as the opened file so
    /// `close_editor_or_confirm`'s `app.active_panel().reload()` has
    /// somewhere real to reload.
    fn open_editor_app(contents: &str) -> (App, PathBuf) {
        let dir = unique_scratch_dir("editor-keymap");
        let file_path = dir.join("file.txt");
        fs::write(&file_path, contents).expect("write test fixture file");

        let editor = Editor::open(file_path.clone(), None).expect("open test fixture file");
        let mut app = test_app(dir);
        app.mode = Mode::Editing(editor);
        (app, file_path)
    }

    mod resolve_editor_key_tests {
        use super::*;

    #[test]
    fn ctrl_s_resolves_to_save() {
        assert_eq!(resolve(ctrl_key('s')), EditorCommand::Save);
    }

    #[test]
    fn ctrl_shift_s_uppercase_still_resolves_to_save() {
        assert_eq!(resolve(ctrl_key('S')), EditorCommand::Save);
    }

    #[test]
    fn esc_resolves_to_close_even_without_ctrl() {
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Close);
    }

    #[test]
    fn plain_s_without_ctrl_is_forwarded_not_save() {
        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Forward);
    }

    #[test]
    fn unmodified_letter_is_forwarded() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Forward);
    }

    #[test]
    fn ctrl_c_is_forwarded_to_edtui_not_handled_here() {
        // Copy/cut/paste are edtui's own concern now (see its custom
        // keymap in editor.rs) -- this module no longer special-cases them.
        assert_eq!(resolve(ctrl_key('c')), EditorCommand::Forward);
    }
    }

    mod resolve_confirm_discard_tests {
        use super::*;

    #[test]
    fn y_or_uppercase_y_confirms_discard() {
        assert_eq!(resolve_confirm_discard(key(KeyCode::Char('y'))), ConfirmDiscardCommand::Discard);
        assert_eq!(resolve_confirm_discard(key(KeyCode::Char('Y'))), ConfirmDiscardCommand::Discard);
    }

    #[test]
    fn n_or_esc_cancels_discard() {
        assert_eq!(resolve_confirm_discard(key(KeyCode::Char('n'))), ConfirmDiscardCommand::Cancel);
        assert_eq!(resolve_confirm_discard(key(KeyCode::Esc)), ConfirmDiscardCommand::Cancel);
    }

    #[test]
    fn other_keys_are_ignored_on_the_discard_prompt() {
        assert_eq!(resolve_confirm_discard(key(KeyCode::Char('x'))), ConfirmDiscardCommand::Ignore);
        assert_eq!(resolve_confirm_discard(key(KeyCode::Enter)), ConfirmDiscardCommand::Ignore);
    }
    }

    mod handle_editor_key_tests {
        use super::*;

    #[test]
    fn handle_editor_key_ctrl_s_saves_and_clears_dirty() {
        let (mut app, path) = open_editor_app("hi\n");
        handle_editor_key(&mut app, ctrl_key('c')).ok(); // no-op sanity: forwarded, doesn't save
        handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();

        handle_editor_key(&mut app, ctrl_key('s')).unwrap();

        let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
        assert!(!editor.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "!hi\n");
    }

    #[test]
    fn handle_editor_key_plain_char_is_forwarded_and_marks_dirty() {
        let (mut app, _path) = open_editor_app("hi\n");

        handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
        assert!(editor.is_dirty());
    }

    #[test]
    fn handle_editor_key_esc_with_no_changes_closes_straight_to_browsing() {
        let (mut app, _path) = open_editor_app("hi\n");

        handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_editor_key_esc_with_unsaved_changes_asks_to_confirm_discard() {
        let (mut app, _path) = open_editor_app("hi\n");
        handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();

        handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::ConfirmDiscard(_)));
    }

    #[test]
    fn handle_editor_key_esc_with_an_active_selection_cancels_the_selection_instead_of_closing() {
        let (mut app, _path) = open_editor_app("hello\n");
        handle_editor_key(&mut app, shift_key(KeyCode::Right)).unwrap();
        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert!(editor.has_selection(), "precondition: a selection should be active");

        handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Mode::Editing(editor) = &app.mode else {
            panic!("Esc should cancel the selection, not close the editor");
        };
        assert!(!editor.has_selection());
    }
    }

    mod handle_confirm_discard_key_tests {
        use super::*;

    #[test]
    fn handle_confirm_discard_key_y_discards_and_returns_to_browsing() {
        let (mut app, _path) = open_editor_app("hi\n");
        handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
        handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::ConfirmDiscard(_)));

        handle_confirm_discard_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_confirm_discard_key_n_cancels_back_into_the_editor_with_changes_intact() {
        let (mut app, _path) = open_editor_app("hi\n");
        handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
        handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        handle_confirm_discard_key(&mut app, key(KeyCode::Char('n'))).unwrap();

        let Mode::Editing(editor) = &app.mode else {
            panic!("Cancel should return to Mode::Editing, not discard");
        };
        assert!(editor.is_dirty(), "the unsaved change should still be there");
    }

    #[test]
    fn handle_confirm_discard_key_ignores_unrelated_keys_and_stays_open() {
        let (mut app, _path) = open_editor_app("hi\n");
        handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
        handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        handle_confirm_discard_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(matches!(app.mode, Mode::ConfirmDiscard(_)));
    }
    }
}
