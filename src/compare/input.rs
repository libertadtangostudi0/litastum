use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};
use crate::editor::{edtui_supports_key, resolve_confirm_discard, ConfirmDiscardCommand};

use super::menu::CompareMenu;

/// A key press while Compare (`Mode::CompareFiles`) is open, at the
/// level `handle_compare_key` needs to care about -- mirrors
/// `editor::editor_keymap::EditorCommand`'s own shape and reasoning
/// almost exactly (see its doc comment for why `Save`/`Forward`/`Ignore`
/// exist as their own variants), with two Compare-specific additions:
/// `ToggleFocus` (`Tab` means "switch pane" everywhere else in this app,
/// not "insert a tab character" -- there's no real conflict, since a
/// literal tab character has no obvious use in either pane's own
/// content) and `NextHunk`/`PreviousHunk` (bound to `Ctrl+Down`/`Ctrl+Up`
/// specifically because `Tab`, this app's usual "next thing" key, is
/// already taken by `ToggleFocus` here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareCommand {
    Close,
    Save,
    ToggleFocus,
    NextHunk,
    PreviousHunk,
    OpenMenu,
    Forward,
    Ignore,
}

fn resolve(key: KeyEvent) -> CompareCommand {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Esc => CompareCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => CompareCommand::Save,
        KeyCode::Tab => CompareCommand::ToggleFocus,
        KeyCode::Down if ctrl => CompareCommand::NextHunk,
        KeyCode::Up if ctrl => CompareCommand::PreviousHunk,
        KeyCode::F(9) => CompareCommand::OpenMenu,
        _ if edtui_supports_key(key.code) => CompareCommand::Forward,
        _ => CompareCommand::Ignore,
    }
}

/// Key handling for `Mode::CompareFiles` -- Compare-level commands
/// (above) are resolved first; anything left over that `edtui` itself
/// understands is forwarded straight into whichever pane currently has
/// focus (`CompareState::focused_mut`), exactly like plain typing
/// already reaches `Editor::input` from `editor::handle_editor_key`.
pub fn handle_compare_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::CompareFiles(state) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "compare key");

    match command {
        CompareCommand::Close => close_compare_or_confirm(app)?,
        CompareCommand::Save => {
            if let Err(err) = state.save_focused() {
                tracing::warn!(%err, "compare: failed to save the focused pane");
            }
        }
        CompareCommand::ToggleFocus => state.toggle_focus(),
        CompareCommand::NextHunk => state.jump_to_next_hunk(),
        CompareCommand::PreviousHunk => state.jump_to_previous_hunk(),
        CompareCommand::OpenMenu => {
            let Mode::CompareFiles(state) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::CompareFiles above");
            };
            app.mode = Mode::CompareMenu(state, CompareMenu::open());
        }
        CompareCommand::Forward => state.focused_mut().input(key),
        CompareCommand::Ignore => {}
    }

    Ok(())
}

/// `Esc` on Compare: closes straight back to browsing if neither pane
/// has unsaved changes, otherwise moves to `Mode::CompareConfirmDiscard`
/// instead of discarding silently -- same shape as
/// `editor::close_editor_or_confirm`, just checking both of Compare's
/// panes (`CompareState::is_dirty`) instead of one editor.
fn close_compare_or_confirm(app: &mut App) -> Result<()> {
    let Mode::CompareFiles(state) = &app.mode else {
        return Ok(());
    };

    if !state.is_dirty() {
        app.mode = Mode::Browsing;
        return Ok(());
    }

    debug!("compare close: unsaved changes, asking to confirm discard");
    let Mode::CompareFiles(state) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::CompareFiles above");
    };
    app.mode = Mode::CompareConfirmDiscard(state);
    Ok(())
}

/// Key handling on Compare's own "discard unsaved changes?" prompt --
/// reuses `editor::resolve_confirm_discard`'s Y/N/Esc shape directly
/// rather than a near-duplicate local copy.
pub fn handle_compare_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let command = resolve_confirm_discard(key);
    debug!(?key, ?command, "compare confirm-discard key");

    match command {
        ConfirmDiscardCommand::Discard => app.mode = Mode::Browsing,
        ConfirmDiscardCommand::Cancel => {
            let Mode::CompareConfirmDiscard(state) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::CompareConfirmDiscard");
            };
            app.mode = Mode::CompareFiles(state);
        }
        ConfirmDiscardCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyEvent;

    use super::*;
    use crate::compare::CompareState;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_compare(left_content: &str, right_content: &str) -> App {
        let dir = unique_scratch_dir("compare-input");
        let left_path = dir.join("left.txt");
        let right_path = dir.join("right.txt");
        std::fs::write(&left_path, left_content).unwrap();
        std::fs::write(&right_path, right_content).unwrap();
        let state = CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap();

        let mut app = test_app(dir);
        app.mode = Mode::CompareFiles(state);
        app
    }

    #[test]
    fn esc_closes_to_browsing_when_nothing_is_dirty() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn esc_asks_to_confirm_discard_when_a_pane_is_dirty() {
        let mut app = app_with_compare("a\n", "b\n");
        handle_compare_key(&mut app, key(KeyCode::Char('!'))).unwrap();

        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::CompareConfirmDiscard(_)));
    }

    #[test]
    fn confirm_discard_y_closes_without_saving() {
        let mut app = app_with_compare("a\n", "b\n");
        handle_compare_key(&mut app, key(KeyCode::Char('!'))).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        handle_compare_confirm_discard_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn confirm_discard_n_returns_to_compare_with_nothing_lost() {
        let mut app = app_with_compare("a\n", "b\n");
        handle_compare_key(&mut app, key(KeyCode::Char('!'))).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        handle_compare_confirm_discard_key(&mut app, key(KeyCode::Char('n'))).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert!(state.is_dirty(), "cancel shouldn't lose the unsaved edit");
    }

    #[test]
    fn f9_opens_the_compare_menu() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::F(9))).unwrap();

        assert!(matches!(app.mode, Mode::CompareMenu(_, _)));
    }

    #[test]
    fn tab_toggles_focus_instead_of_typing_a_tab_character() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::Tab)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert_eq!(state.focus, crate::compare::Side::Right);
    }

    #[test]
    fn ctrl_down_jumps_to_the_next_hunk() {
        let mut app = app_with_compare("a\nb\nc\n", "a\nx\nc\n");

        handle_compare_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert_eq!(state.left.cursor().row, 1);
    }

    #[test]
    fn plain_typing_is_forwarded_into_the_focused_pane() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert!(state.left.text().starts_with('z'), "should have typed into the focused (left) pane");
        assert_eq!(state.right.text(), "b\n", "the unfocused pane should be untouched");
    }

    #[test]
    fn ctrl_s_saves_the_focused_pane() {
        let mut app = app_with_compare("a\n", "b\n");
        handle_compare_key(&mut app, key(KeyCode::Char('!'))).unwrap();

        handle_compare_key(&mut app, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert!(!state.left.is_dirty());
    }
}
