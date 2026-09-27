use std::path::Path;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, warn};

use crate::app::{App, Mode};
use crate::editor::Editor;

use super::super::export::export_results;

/// Key handling for `FindFilePhase::Results` -- `Up`/`Down` move the
/// selection, `Shift+Up`/`Down` mark the current row and move (same
/// `Panel::toggle_mark_move_up`/`_down` convention, see
/// `FindFileState::toggle_mark_move_down`'s own doc comment for why
/// marks here are indexed rather than name-keyed), `Alt+F5` compares
/// exactly two marked results, `Enter` opens the selected result and
/// closes the popup, `Tab` does the same navigation but leaves the
/// popup open, `F4` opens it in the built-in editor, `Ctrl+S` exports
/// the full list, `Ctrl+C` copies the highlighted result's own full
/// path to the real OS clipboard -- reported directly as a real gap:
/// there was no way to get a result's path out of this popup at all
/// short of exporting the *entire* list to a file. `Esc` is handled one
/// level up, in `handle_find_file_key`, ahead of this dispatch
/// entirely.
pub(super) fn handle_results_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let Mode::FindFile(state) = &mut app.mode else {
                unreachable!("handle_find_file_key only dispatches here while Mode::FindFile(_) is active");
            };
            state.toggle_mark_move_up();
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let Mode::FindFile(state) = &mut app.mode else {
                unreachable!("handle_find_file_key only dispatches here while Mode::FindFile(_) is active");
            };
            state.toggle_mark_move_down();
        }
        KeyCode::Up => {
            let Mode::FindFile(state) = &mut app.mode else {
                unreachable!("handle_find_file_key only dispatches here while Mode::FindFile(_) is active");
            };
            state.selected = state.selected.saturating_sub(1);
        }
        KeyCode::Down => {
            let Mode::FindFile(state) = &mut app.mode else {
                unreachable!("handle_find_file_key only dispatches here while Mode::FindFile(_) is active");
            };
            if state.selected + 1 < state.results.len() {
                state.selected += 1;
            }
        }
        KeyCode::Enter => return open_selected_result(app),
        KeyCode::Tab => return goto_selected_result_directory(app),
        KeyCode::F(4) => return edit_selected_result(app),
        KeyCode::F(5) if key.modifiers.contains(KeyModifiers::ALT) => {
            return compare_marked_results(app);
        }
        KeyCode::Char('s' | 'S') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return run_export(app);
        }
        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            copy_selected_result_path(app);
        }
        _ => {}
    }

    Ok(())
}

/// `Alt+F5` on the results popup: compares the two marked results,
/// same `compare::CompareState` this app's own panel-to-panel `Alt+F5`
/// (`command_line::browsing::handle_browsing_key`) already opens --
/// requested directly, since results found by a search can easily live
/// in two unrelated directories with no convenient way to get both of
/// them into the two panels first. A silent no-op unless exactly two
/// results are marked, or either resolved path can't actually be
/// compared (a directory result, or a read failure) -- same convention
/// `compare_targets` already follows for the panel version of this
/// action.
fn compare_marked_results(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let Some((left_path, right_path)) = state.two_marked_results() else {
        return Ok(());
    };
    if left_path.is_dir() || right_path.is_dir() {
        return Ok(());
    }

    let syntax_theme = app.syntax_theme.clone();
    if let Ok(compare_state) = crate::compare::CompareState::open(left_path, right_path, syntax_theme, app.editor_keymap_mode) {
        app.mode = Mode::CompareFiles(compare_state);
    }
    Ok(())
}

/// `Ctrl+C` on a result: copies its own full path (not just the
/// directory `Tab`/`Enter` navigate to) to the real OS clipboard --
/// same `arboard` dependency `editor::clipboard` and
/// `explorer::confirm`'s own transfer-field copy already use. A missing
/// clipboard or a failed OS call is only logged, same "don't fail the
/// keystroke over it" rule those two already follow.
fn copy_selected_result_path(app: &mut App) {
    let Mode::FindFile(state) = &app.mode else {
        return;
    };
    let Some(path) = selected_result_path(state) else {
        return;
    };

    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            if let Err(err) = clipboard.set_text(path) {
                warn!(%err, "find file: clipboard set_text failed");
            }
        }
        Err(err) => warn!(%err, "find file: clipboard unavailable"),
    }
}

/// The highlighted result's own full path, formatted for the clipboard
/// -- split out as a pure function so this has real unit coverage
/// without touching the real OS clipboard (this codebase deliberately
/// avoids that elsewhere too, see `explorer::confirm::selected_text`'s
/// own doc comment).
fn selected_result_path(state: &crate::explorer::FindFileState) -> Option<String> {
    state.results.get(state.selected).map(|path| path.display().to_string())
}

/// `Ctrl+S` on the results popup: writes `export_results` and records
/// the outcome (success or failure, both — there's no other
/// status-bar surface to report a failure on yet) in
/// `state.export_message` for `draw_results` to show.
fn run_export(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let message = match export_results(state) {
        Ok(path) => {
            debug!(path = %path.display(), "find file: exported results");
            ("Exported to:".to_string(), path.display().to_string())
        }
        Err(err) => {
            debug!(%err, "find file: export failed");
            ("Export failed:".to_string(), err.to_string())
        }
    };

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("just matched Mode::FindFile above");
    };
    state.export_message = Some(message);
    Ok(())
}

/// `Enter` on a result: closes the popup and moves the active panel to
/// the result's directory with it selected, same as double-clicking a
/// search hit in a real file manager would.
fn open_selected_result(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let Some(path) = state.results.get(state.selected).cloned() else {
        return Ok(());
    };

    app.mode = Mode::Browsing;
    navigate_active_panel_to_result(app, &path)
}

/// `Tab` on a result: the same directory navigation `open_selected_result`
/// (`Enter`) performs, but leaves the popup open in
/// `FindFilePhase::Results` instead of closing it -- requested directly
/// so browsing further results with `Up`/`Down` (or pressing `Tab`
/// again on a different one) keeps updating the panel in the background
/// without having to reopen Find file each time.
fn goto_selected_result_directory(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let Some(path) = state.results.get(state.selected).cloned() else {
        return Ok(());
    };

    navigate_active_panel_to_result(app, &path)
}

/// Moves the active panel to `path`'s own directory and, if it's still
/// listed there under its own name, selects it -- shared by
/// `open_selected_result` and `goto_selected_result_directory`, which
/// only differ in whether `app.mode` also switches back to
/// `Mode::Browsing` afterward.
fn navigate_active_panel_to_result(app: &mut App, path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    app.panels[app.active].path = parent.to_path_buf();
    app.panels[app.active].reload()?;
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if let Some(index) = app.panels[app.active].entries.iter().position(|entry| entry.name == name) {
            app.panels[app.active].selected = index;
        }
    }
    Ok(())
}

/// `F4` on a result: opens it in the built-in editor, same as `F4` from
/// the browser (`explorer::command::open_editor`) -- does nothing for a
/// directory result (search results can include directory name matches
/// too, not just files) or a file that fails to load as UTF-8 text,
/// same as that function.
///
/// The results list itself isn't dropped, just set aside
/// (`app.editor_return_to`) -- reported directly as a real gap:
/// finishing the edit used to always land back in plain browsing,
/// losing the search results even though nothing about them was
/// actually done with yet. `editor_keymap::return_from_editor` restores
/// `Mode::FindFile` from it once the editor genuinely closes (`Esc`
/// with no unsaved changes, or discarding them) -- moved via
/// `mem::replace` rather than cloned, so a large result set (the very
/// case the popup's own scrolling exists for) doesn't get deep-copied
/// just to park it here.
fn edit_selected_result(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let Some(path) = state.results.get(state.selected).cloned() else {
        return Ok(());
    };
    if path.is_dir() {
        return Ok(());
    }

    let syntax_theme = app.syntax_theme.clone();
    let Ok(editor) = Editor::open(path, syntax_theme, app.editor_keymap_mode) else {
        return Ok(());
    };

    let Mode::FindFile(state) = std::mem::replace(&mut app.mode, Mode::Editing(editor)) else {
        unreachable!("just matched Mode::FindFile above");
    };
    app.editor_return_to = Some(state);
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::super::test_support::app_with_find_file;
    use super::super::super::state::FindFilePhase;
    use super::*;
    use crate::explorer::FindFileState;
    use crate::test_support::key;

    #[test]
    fn enter_on_a_result_navigates_the_active_panel_and_selects_it() {
        let mut app = app_with_find_file(FindFileState::new());
        let target_dir = app.panels[0].path.join("nested");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("target.txt"), b"hi").unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![target_dir.join("target.txt")];
        app.mode = Mode::FindFile(results_state);

        handle_results_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert_eq!(app.panels[0].path, target_dir);
        let selected_name = &app.panels[0].entries[app.panels[0].selected].name;
        assert_eq!(selected_name, "target.txt");
    }

    #[test]
    fn up_and_down_move_the_result_selection() {
        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut app = app_with_find_file(results_state);

        handle_results_key(&mut app, key(KeyCode::Down)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.selected, 1);
    }

    /// Regression coverage for the real request: `Shift+Down`/`Up`
    /// should mark the current result and move, the same way
    /// `Panel`'s own multi-select already works, rather than plain
    /// `Up`/`Down`'s unmarked navigation.
    #[test]
    fn shift_down_marks_the_current_result_and_moves() {
        use crate::test_support::shift_key;

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")];
        let mut app = app_with_find_file(results_state);

        handle_results_key(&mut app, shift_key(KeyCode::Down)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.marked, std::collections::HashSet::from([0]));
        assert_eq!(state.selected, 1);
    }

    /// Requested directly: select two results found by a search (which,
    /// unlike a panel's own marks, can easily live in unrelated
    /// directories) with `Shift+Up`/`Down`, then compare them with
    /// `Alt+F5`, the same compare feature the panel-to-panel binding
    /// already opens.
    #[test]
    fn alt_f5_compares_the_two_marked_results() {
        use crate::test_support::shift_key;

        let mut app = app_with_find_file(FindFileState::new());
        let left = app.panels[0].path.join("left.rs");
        let right = app.panels[0].path.join("right.rs");
        fs::write(&left, "fn left() {}\n").unwrap();
        fs::write(&right, "fn right() {}\n").unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![left, right];
        app.mode = Mode::FindFile(results_state);

        handle_results_key(&mut app, shift_key(KeyCode::Down)).unwrap(); // marks index 0, moves to 1
        handle_results_key(&mut app, shift_key(KeyCode::Down)).unwrap(); // marks index 1

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(5), KeyModifiers::ALT)).unwrap();

        assert!(matches!(app.mode, Mode::CompareFiles(_)), "Alt+F5 with exactly two results marked should open the compare view");
    }

    /// A silent no-op, matching `compare_targets`'s own convention --
    /// with fewer (or more) than exactly two results marked there's
    /// nothing well-defined to compare.
    #[test]
    fn alt_f5_does_nothing_unless_exactly_two_results_are_marked() {
        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut app = app_with_find_file(results_state);

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(5), KeyModifiers::ALT)).unwrap();

        assert!(matches!(app.mode, Mode::FindFile(_)), "nothing marked yet, so Alt+F5 should do nothing");
    }

    /// Regression coverage for the real request: `Tab` should perform
    /// the same directory navigation `Enter` does, but leave the popup
    /// open so further results can still be browsed.
    #[test]
    fn tab_on_a_result_navigates_the_active_panel_but_keeps_the_popup_open() {
        let mut app = app_with_find_file(FindFileState::new());
        let target_dir = app.panels[0].path.join("nested");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("target.txt"), b"hi").unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![target_dir.join("target.txt")];
        app.mode = Mode::FindFile(results_state);

        handle_results_key(&mut app, key(KeyCode::Tab)).unwrap();

        assert_eq!(app.panels[0].path, target_dir);
        let selected_name = &app.panels[0].entries[app.panels[0].selected].name;
        assert_eq!(selected_name, "target.txt");
        let Mode::FindFile(state) = &app.mode else {
            panic!("Tab should leave the popup open, unlike Enter");
        };
        assert_eq!(state.phase, FindFilePhase::Results);
    }

    /// Regression coverage for the real request: `F4` should open the
    /// selected result in the built-in editor, same as `F4` from the
    /// browser.
    #[test]
    fn f4_on_a_result_opens_it_in_the_editor() {
        let mut app = app_with_find_file(FindFileState::new());
        let target = app.panels[0].path.join("target.txt");
        fs::write(&target, b"hello").unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![target];
        app.mode = Mode::FindFile(results_state);

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.mode, Mode::Editing(_)));
    }

    /// Regression coverage for the real follow-up request: closing the
    /// editor after `F4`-from-Find-file should return to the results
    /// popup with its results intact, not drop back to plain
    /// browsing -- the search itself isn't "done with" just because one
    /// result got opened.
    #[test]
    fn closing_the_editor_after_f4_from_find_file_returns_to_the_results_popup() {
        let mut app = app_with_find_file(FindFileState::new());
        let target = app.panels[0].path.join("target.txt");
        fs::write(&target, b"hello").unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![target];
        app.mode = Mode::FindFile(results_state);

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();
        assert!(matches!(app.mode, Mode::Editing(_)), "sanity");

        crate::editor::handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Mode::FindFile(state) = &app.mode else {
            panic!("closing the editor should return to Mode::FindFile, not Mode::Browsing");
        };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results.len(), 1);
    }

    /// `F4` on a directory result (search results can include directory
    /// name matches too) should do nothing, matching `F4`'s own
    /// behavior on a directory in the browser.
    #[test]
    fn f4_on_a_directory_result_does_nothing() {
        let mut app = app_with_find_file(FindFileState::new());
        let target_dir = app.panels[0].path.join("a_dir");
        fs::create_dir_all(&target_dir).unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![target_dir];
        app.mode = Mode::FindFile(results_state);

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.mode, Mode::FindFile(_)));
    }

    mod selected_result_path_tests {
        use super::*;

        #[test]
        fn returns_the_highlighted_results_own_full_path() {
            let mut state = FindFileState::new();
            state.results = vec![PathBuf::from(r"W:\WorkCopies\trunk\a.cpp"), PathBuf::from(r"W:\WorkCopies\trunk\b.cpp")];
            state.selected = 1;

            assert_eq!(selected_result_path(&state), Some(r"W:\WorkCopies\trunk\b.cpp".to_string()));
        }

        #[test]
        fn no_results_means_nothing_to_copy() {
            let state = FindFileState::new();
            assert_eq!(selected_result_path(&state), None);
        }
    }
}
