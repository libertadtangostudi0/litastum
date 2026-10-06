use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::editor::{cancel_selection, edtui_supports_key, text_key};
use crate::notice::Notice;
use crate::yes_no::{self, Answer};

use super::menu::open_compare_menu;

/// A key in Compare, like `editor_keymap::EditorCommand` plus:
/// `ToggleFocus` (`Tab` switches panes, as elsewhere in the app) and
/// `NextHunk`/`PreviousHunk` on `Ctrl+Down`/`Ctrl+Up` and on `F8`/`F7`
/// (TortoiseMerge/`merge.exe` convention, requested).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareCommand {
    Close,
    Save,
    ToggleFocus,
    EditPath,
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
        KeyCode::Char('l' | 'L') if ctrl => CompareCommand::EditPath,
        KeyCode::Down if ctrl => CompareCommand::NextHunk,
        KeyCode::Up if ctrl => CompareCommand::PreviousHunk,
        KeyCode::F(8) => CompareCommand::NextHunk,
        KeyCode::F(7) => CompareCommand::PreviousHunk,
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
pub fn handle_compare_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Mode::CompareFiles(state) = &mut app.mode else {
        return Ok(Effect::None);
    };

    if state.path_edit.is_some() {
        if let Err(err) = state.path_edit_key(key) {
            app.notice = Some(Notice::error(format!("Can't open: {err}")));
        }
        return Ok(Effect::None);
    }

    let command = resolve(key);
    debug!(?key, ?command, "compare key");

    match command {
        CompareCommand::Close if cancel_selection(state.focused_mut(), key) => {}
        CompareCommand::Close => close_compare_or_confirm(app)?,
        CompareCommand::Save => {
            if let Err(err) = state.save_focused() {
                tracing::warn!(%err, "compare: failed to save the focused pane");
                app.notice = Some(Notice::error(format!("Save failed: {err}")));
            }
        }
        CompareCommand::ToggleFocus => state.toggle_focus(),
        CompareCommand::EditPath => state.start_path_edit(),
        CompareCommand::NextHunk => state.jump_to_next_hunk(),
        CompareCommand::PreviousHunk => state.jump_to_previous_hunk(),
        CompareCommand::OpenMenu => app.overlay = Some(Overlay::CompareMenu(open_compare_menu())),
        CompareCommand::Forward => text_key(state.focused_mut(), key),
        CompareCommand::Ignore => {}
    }

    Ok(Effect::None)
}

/// `Esc` on Compare: closes straight back to browsing if neither pane
/// has unsaved changes, otherwise opens `Overlay::ConfirmDiscard`
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
    app.overlay = Some(Overlay::ConfirmDiscard);
    Ok(())
}

/// Key handling on the "discard unsaved changes?" prompt over Compare or
/// the conflict resolver -- either way, `Yes` drops back to browsing.
pub fn handle_compare_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let answer = yes_no::answer(key);
    debug!(?key, ?answer, "compare confirm-discard key");

    match answer {
        Answer::Yes => {
            app.overlay = None;
            app.mode = Mode::Browsing;
        }
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }

    Ok(Effect::None)
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

        assert!(matches!(app.overlay, Some(Overlay::ConfirmDiscard)));
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

        assert!(matches!(app.overlay, Some(Overlay::CompareMenu(_))));
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

    /// `F7`/`F8` are the same `NextHunk`/`PreviousHunk` commands
    /// `Ctrl+Down`/`Ctrl+Up` already drive -- requested directly to
    /// match TortoiseMerge/`merge.exe`'s own next/previous-difference
    /// convention, not a replacement for the existing binding.
    #[test]
    fn f8_jumps_to_the_next_hunk_same_as_ctrl_down() {
        let mut app = app_with_compare("a\nb\nc\n", "a\nx\nc\n");

        handle_compare_key(&mut app, key(KeyCode::F(8))).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert_eq!(state.left.cursor().row, 1);
    }

    #[test]
    fn f7_jumps_to_the_previous_hunk_same_as_ctrl_up() {
        let mut app = app_with_compare("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        handle_compare_key(&mut app, key(KeyCode::F(8))).unwrap();
        handle_compare_key(&mut app, key(KeyCode::F(8))).unwrap();

        handle_compare_key(&mut app, key(KeyCode::F(7))).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert_eq!(state.left.cursor().row, 1, "should land back on the first hunk, not stay on the second");
    }

    #[test]
    fn ctrl_l_edits_the_path_and_keys_go_into_the_field() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Char('z'))).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("Esc closed the field, not Compare") };
        assert!(state.path_edit.is_none());
        assert_eq!(state.left.text(), "a\n", "typing went into the field, not the file");
    }

    #[test]
    fn plain_typing_is_forwarded_into_the_focused_pane() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert!(state.left.text().starts_with('z'), "should have typed into the focused (left) pane");
        assert_eq!(state.right.text(), "b\n", "the unfocused pane should be untouched");
    }

    fn left_text(app: &App) -> String {
        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        state.left.text()
    }

    fn ctrl_shift(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
    }

    /// Reported: Ctrl+Shift+arrows didn't select in Compare -- its keys
    /// went to `Editor::input`, past F4's word selection.
    #[test]
    fn ctrl_shift_right_selects_a_word_as_in_the_editor() {
        let mut app = app_with_compare("hello world\n", "x\n");

        handle_compare_key(&mut app, ctrl_shift(KeyCode::Right)).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Char('X'))).unwrap();

        assert_eq!(left_text(&app), "X world\n", "the word was selected, then typed over");
    }

    #[test]
    fn ctrl_a_selects_everything_in_the_pane() {
        let mut app = app_with_compare("one\ntwo\n", "x\n");

        handle_compare_key(&mut app, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Char('Z'))).unwrap();

        assert_eq!(left_text(&app).trim_end(), "Z");
    }

    /// Esc with a selection cancels it, as in F4, instead of closing
    /// Compare.
    #[test]
    fn esc_cancels_a_selection_before_it_closes_compare() {
        let mut app = app_with_compare("hello\n", "x\n");
        handle_compare_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT)).unwrap();

        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("Compare closed instead") };
        assert!(!state.left.has_selection());
        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing), "the next Esc closes");
    }

    /// The rest of selection editing goes through the same `Editor` as
    /// F4: Shift+End and typing over, Shift+Left and Backspace.
    #[test]
    fn selection_editing_matches_the_editor() {
        let mut app = app_with_compare("hello world\n", "x\n");
        handle_compare_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT)).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Char('Y'))).unwrap();
        assert_eq!(left_text(&app), "Y\n");

        handle_compare_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap();
        handle_compare_key(&mut app, key(KeyCode::Backspace)).unwrap();
        assert_eq!(left_text(&app), "\n");
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
