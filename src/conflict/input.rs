use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::editor::{cancel_selection, edtui_supports_key, text_key};
use crate::notice::Notice;

use super::files::ConflictFiles;
use super::state::ConflictState;

/// `Alt+F5` on the four conflict files, from the panels or Find file.
/// A file that can't be opened (not UTF-8, gone) shows as a notice.
pub fn open_resolver(app: &mut App, files: ConflictFiles) {
    let syntax_theme = app.syntax_theme.clone();
    match ConflictState::open(files, syntax_theme, app.settings.editor_keymap_mode) {
        Ok(state) => app.mode = Mode::ResolveConflict(Box::new(state)),
        Err(err) => {
            tracing::warn!(%err, "conflict: failed to open the resolver");
            app.notice = Some(Notice::error(format!("Can't open the conflict: {err}")));
        }
    }
}

/// A key in the resolver: `Tab`/`Shift+Tab` move between the five panes;
/// `F7`/`F8` (and `Alt+Up`/`Down`) step through the focused pane's
/// changes, and the result's conflict markers (`ConflictState::stops`);
/// `Ctrl+Up`/`Down` move by blocks of code, as in F4 (requested).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConflictCommand {
    Close,
    Save,
    SaveAs,
    NextPane,
    PreviousPane,
    EditPath,
    Next,
    Previous,
    Forward,
    Ignore,
}

fn resolve(key: KeyEvent) -> ConflictCommand {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Esc => ConflictCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => ConflictCommand::Save,
        // Ctrl+F2 edits the path, as on a panel's title; above plain F2 (save).
        KeyCode::F(2) if ctrl => ConflictCommand::EditPath,
        KeyCode::F(2) if key.modifiers.contains(KeyModifiers::SHIFT) => ConflictCommand::SaveAs,
        KeyCode::F(2) => ConflictCommand::Save,
        KeyCode::Tab => ConflictCommand::NextPane,
        KeyCode::BackTab => ConflictCommand::PreviousPane,
        KeyCode::Char('l' | 'L') if ctrl => ConflictCommand::EditPath,
        KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => ConflictCommand::Next,
        KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => ConflictCommand::Previous,
        KeyCode::F(8) => ConflictCommand::Next,
        KeyCode::F(7) => ConflictCommand::Previous,
        _ if edtui_supports_key(key.code) => ConflictCommand::Forward,
        _ => ConflictCommand::Ignore,
    }
}

/// Key handling for `Mode::ResolveConflict`: resolver commands first,
/// anything `edtui` understands goes to the focused pane.
pub fn handle_conflict_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Mode::ResolveConflict(state) = &mut app.mode else {
        return Ok(Effect::None);
    };

    if state.is_editing_path() {
        if let Err(err) = state.path_edit_key(key) {
            app.notice = Some(Notice::error(err.to_string()));
        }
        return Ok(Effect::None);
    }

    let command = resolve(key);
    debug!(?key, ?command, "conflict key");

    match command {
        ConflictCommand::Close if cancel_selection(state.focused_mut(), key) => {}
        ConflictCommand::Close if state.is_dirty() => app.overlay = Some(Overlay::ConfirmDiscard),
        ConflictCommand::Close => app.mode = Mode::Browsing,
        ConflictCommand::Save => {
            if let Err(err) = state.save_focused() {
                tracing::warn!(%err, "conflict: failed to save the focused pane");
                app.notice = Some(Notice::error(format!("Save failed: {err}")));
            }
        }
        ConflictCommand::NextPane => state.focus_next(),
        ConflictCommand::PreviousPane => state.focus_previous(),
        ConflictCommand::EditPath => state.start_path_edit(),
        ConflictCommand::SaveAs => state.start_save_as(),
        ConflictCommand::Next => state.jump_to_next(),
        ConflictCommand::Previous => state.jump_to_previous(),
        ConflictCommand::Forward => text_key(state.focused_mut(), key),
        ConflictCommand::Ignore => {}
    }

    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::state::tests::open_conflict;
    use crate::conflict::Pane;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_conflict() -> App {
        let mut app = test_app(unique_scratch_dir("conflict-input"));
        app.mode = Mode::ResolveConflict(Box::new(open_conflict()));
        app
    }

    fn state(app: &App) -> &crate::conflict::ConflictState {
        let Mode::ResolveConflict(state) = &app.mode else { panic!("expected Mode::ResolveConflict") };
        state
    }

    #[test]
    fn typing_goes_into_the_result_pane_first() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        assert!(state(&app).result.text().starts_with('z'));
        assert_eq!(state(&app).working.text(), "one\nmine\n");
    }

    #[test]
    fn tab_moves_to_the_next_pane_instead_of_typing() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, key(KeyCode::Tab)).unwrap();

        assert_eq!(state(&app).focus, Pane::Theirs);
        assert!(!state(&app).is_dirty());
    }

    #[test]
    fn f8_in_the_result_pane_jumps_to_the_conflict() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, key(KeyCode::F(8))).unwrap();

        assert_eq!(state(&app).result.cursor().row, 1, "the <<<<<<< row");
    }

    /// Requested: the changes on `Alt+Up`/`Down` too, as in Compare.
    #[test]
    fn alt_down_steps_to_the_conflict_like_f8() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)).unwrap();

        assert_eq!(state(&app).result.cursor().row, 1, "the <<<<<<< row");
    }

    /// Requested: `Ctrl+Up`/`Down` move by blocks of code, as in F4 --
    /// here past the conflict, the result being one block.
    #[test]
    fn ctrl_down_moves_by_blocks_as_in_the_editor() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)).unwrap();

        assert!(state(&app).result.cursor().row >= 5, "to the last line, no next block: {:?}", state(&app).result.cursor());
    }

    #[test]
    fn f8_steps_through_the_bottom_compare_once_it_has_focus() {
        let mut app = app_with_conflict();
        handle_conflict_key(&mut app, key(KeyCode::Tab)).unwrap();
        handle_conflict_key(&mut app, key(KeyCode::Tab)).unwrap();

        handle_conflict_key(&mut app, key(KeyCode::F(8))).unwrap();

        assert_eq!(state(&app).incoming.left.cursor().row, 1, "\"base\" -> \"theirs\"");
    }

    #[test]
    fn esc_in_the_path_field_closes_only_the_field() {
        let mut app = app_with_conflict();
        handle_conflict_key(&mut app, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)).unwrap();
        handle_conflict_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        handle_conflict_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(!state(&app).is_editing_path());
        assert!(!state(&app).is_dirty(), "the z went into the field");
    }

    #[test]
    fn esc_closes_when_nothing_is_dirty() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn esc_asks_to_confirm_discard_when_a_pane_is_dirty() {
        let mut app = app_with_conflict();
        handle_conflict_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        handle_conflict_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::ConfirmDiscard)));
        assert!(matches!(app.mode, Mode::ResolveConflict(_)));
    }

    /// The resolver's panes are editors like Compare's: word selection
    /// works, and Esc cancels a selection before it closes anything.
    #[test]
    fn word_selection_and_esc_work_in_the_result_pane() {
        let mut app = app_with_conflict();

        handle_conflict_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap();
        assert!(state(&app).result.has_selection());

        handle_conflict_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::ResolveConflict(_)), "Esc only cancelled the selection");
        assert!(!state(&app).result.has_selection());
    }

    /// Requested: Ctrl+F2 edits the path in every pane, as on the panels.
    #[test]
    fn ctrl_f2_edits_the_focused_panes_path_in_every_pane() {
        let mut app = app_with_conflict();
        // working, result, theirs, the bottom Compare
        for _ in 0..4 {
            handle_conflict_key(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::CONTROL)).unwrap();
            assert!(state(&app).is_editing_path(), "pane {:?}", state(&app).focus);
            handle_conflict_key(&mut app, key(KeyCode::Esc)).unwrap();
            handle_conflict_key(&mut app, key(KeyCode::Tab)).unwrap();
        }
    }

    #[test]
    fn f2_saves_the_focused_pane() {
        let mut app = app_with_conflict();
        handle_conflict_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        handle_conflict_key(&mut app, key(KeyCode::F(2))).unwrap();

        assert!(!state(&app).is_dirty());
    }

    #[test]
    fn ctrl_s_saves_the_focused_pane() {
        let mut app = app_with_conflict();
        handle_conflict_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        handle_conflict_key(&mut app, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();

        assert!(!state(&app).is_dirty());
    }
}
