use std::path::Path;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, warn};

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::editor::Editor;
use crate::notice::Notice;

use super::super::export::export_results;

/// Keys on the results: `Up`/`Down` move, `Shift+Up`/`Down` mark and move,
/// `Alt+F5` compares two marked results, `Enter` opens and closes the
/// popup, `Tab` opens and keeps it, `F4` edits, `Ctrl+S` exports,
/// `Ctrl+C` copies the highlighted path. `Esc` is handled one level up.
/// History: docs/history/find-file-search.md.
pub(super) fn handle_results_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    match key.code {
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let Some(Overlay::FindFile(state)) = &mut app.overlay else {
                unreachable!("handle_find_file_key only dispatches here while Overlay::FindFile(_) is active");
            };
            state.toggle_mark_move_up();
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let Some(Overlay::FindFile(state)) = &mut app.overlay else {
                unreachable!("handle_find_file_key only dispatches here while Overlay::FindFile(_) is active");
            };
            state.toggle_mark_move_down();
        }
        KeyCode::Up => {
            let Some(Overlay::FindFile(state)) = &mut app.overlay else {
                unreachable!("handle_find_file_key only dispatches here while Overlay::FindFile(_) is active");
            };
            state.selected = state.selected.saturating_sub(1);
        }
        KeyCode::Down => {
            let Some(Overlay::FindFile(state)) = &mut app.overlay else {
                unreachable!("handle_find_file_key only dispatches here while Overlay::FindFile(_) is active");
            };
            if state.selected + 1 < state.results.len() {
                state.selected += 1;
            }
        }
        KeyCode::Enter => open_selected_result(app)?,
        KeyCode::Tab => goto_selected_result_directory(app)?,
        KeyCode::F(4) => edit_selected_result(app)?,
        KeyCode::F(5) if key.modifiers.contains(KeyModifiers::ALT) => compare_marked_results(app)?,
        KeyCode::Char('s' | 'S') if key.modifiers.contains(KeyModifiers::CONTROL) => run_export(app)?,
        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            copy_selected_result_path(app);
        }
        _ => {}
    }

    Ok(Effect::None)
}

/// `Alt+F5`: the conflict resolver on an SVN conflict's four marked
/// results, as in the panels; otherwise compares the two marked results
/// -- search hits often live in unrelated directories. No-op unless
/// exactly two are marked and both can be compared.
fn compare_marked_results(app: &mut App) -> Result<()> {
    let Some(Overlay::FindFile(state)) = &app.overlay else {
        return Ok(());
    };
    if let Some(files) = crate::conflict::detect(&state.marked_results()) {
        app.overlay = None;
        crate::conflict::open_resolver(app, files);
        return Ok(());
    }
    let Some((left_path, right_path)) = state.two_marked_results() else {
        return Ok(());
    };
    if left_path.is_dir() || right_path.is_dir() {
        return Ok(());
    }

    let syntax_theme = app.syntax_theme.clone();
    if let Ok(compare_state) = crate::compare::CompareState::open(left_path, right_path, syntax_theme, app.settings.editor_keymap_mode) {
        app.overlay = None;
        app.mode = Mode::CompareFiles(compare_state);
    }
    Ok(())
}

/// `Ctrl+C` on a result: copies its own full path (not just the
/// directory `Tab`/`Enter` navigate to) to the real OS clipboard --
/// same `arboard` dependency `editor::clipboard` and
/// `explorer::confirm`'s own transfer-field copy already use. The result
/// shows as a notice ("Path copied", or the error).
fn copy_selected_result_path(app: &mut App) {
    let Some(Overlay::FindFile(state)) = &app.overlay else {
        return;
    };
    let Some(path) = selected_result_path(state) else {
        return;
    };

    let result = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(path));
    app.notice = Some(match result {
        Ok(()) => Notice::info("Path copied"),
        Err(err) => {
            warn!(%err, "find file: couldn't copy the path to the clipboard");
            Notice::error(format!("Copy failed: {err}"))
        }
    });
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
/// the outcome (success or failure) in `state.export_message`, shown in
/// the popup itself -- the exported path needs a row of its own.
fn run_export(app: &mut App) -> Result<()> {
    let Some(Overlay::FindFile(state)) = &app.overlay else {
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

    let Some(Overlay::FindFile(state)) = &mut app.overlay else {
        unreachable!("just matched Overlay::FindFile above");
    };
    state.export_message = Some(message);
    Ok(())
}

/// `Enter` on a result: closes the popup and moves the active panel to
/// the result's directory with it selected, same as double-clicking a
/// search hit in a real file manager would.
fn open_selected_result(app: &mut App) -> Result<()> {
    let Some(Overlay::FindFile(state)) = &app.overlay else {
        return Ok(());
    };
    let Some(path) = state.results.get(state.selected).cloned() else {
        return Ok(());
    };

    app.overlay = None;
    navigate_active_panel_to_result(app, &path)
}

/// `Tab` on a result: the same directory navigation `open_selected_result`
/// (`Enter`) performs, but leaves the popup open in
/// `FindFilePhase::Results` instead of closing it -- requested directly
/// so browsing further results with `Up`/`Down` (or pressing `Tab`
/// again on a different one) keeps updating the panel in the background
/// without having to reopen Find file each time.
fn goto_selected_result_directory(app: &mut App) -> Result<()> {
    let Some(Overlay::FindFile(state)) = &app.overlay else {
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

/// `F4` on a result: opens it in the editor (not a directory, not
/// non-UTF-8). The results are parked in `app.editor_return_to` (moved,
/// not cloned) and restored when the editor closes, instead of dropping
/// back to plain browsing. History: docs/history/find-file-search.md.
fn edit_selected_result(app: &mut App) -> Result<()> {
    let Some(Overlay::FindFile(state)) = &app.overlay else {
        return Ok(());
    };
    let Some(path) = state.results.get(state.selected).cloned() else {
        return Ok(());
    };
    if path.is_dir() {
        return Ok(());
    }

    let syntax_theme = app.syntax_theme.clone();
    let Ok(editor) = Editor::open(path, syntax_theme, app.settings.editor_keymap_mode) else {
        return Ok(());
    };

    let Some(Overlay::FindFile(state)) = app.overlay.take() else {
        unreachable!("just matched Overlay::FindFile above");
    };
    app.mode = Mode::Editing(editor);
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
        app.overlay = Some(Overlay::FindFile(results_state));

        handle_results_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
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

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
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

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
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
        app.overlay = Some(Overlay::FindFile(results_state));

        handle_results_key(&mut app, shift_key(KeyCode::Down)).unwrap(); // marks index 0, moves to 1
        handle_results_key(&mut app, shift_key(KeyCode::Down)).unwrap(); // marks index 1

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(5), KeyModifiers::ALT)).unwrap();

        assert!(matches!(app.mode, Mode::CompareFiles(_)), "Alt+F5 with exactly two results marked should open the compare view");
        assert!(app.overlay.is_none(), "the Find file popup must not stay open over Compare");
    }

    /// The resolver opens from Find file too: a search for the file's name
    /// lists all four conflict files -- the panel route was the only one
    /// at first, and Alt+F5 on the marked results did nothing.
    #[test]
    fn alt_f5_on_four_marked_conflict_results_opens_the_resolver() {
        let mut app = app_with_find_file(FindFileState::new());
        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = crate::conflict::state_tests::write_conflict_files(&app.panels[0].path);
        results_state.marked = (0..4).collect();
        app.overlay = Some(Overlay::FindFile(results_state));

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(5), KeyModifiers::ALT)).unwrap();

        assert!(matches!(app.mode, Mode::ResolveConflict(_)));
        assert!(app.overlay.is_none(), "the Find file popup must not stay open over the resolver");
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

        assert!(matches!(app.overlay, Some(Overlay::FindFile(_))), "nothing marked yet, so Alt+F5 should do nothing");
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
        app.overlay = Some(Overlay::FindFile(results_state));

        handle_results_key(&mut app, key(KeyCode::Tab)).unwrap();

        assert_eq!(app.panels[0].path, target_dir);
        let selected_name = &app.panels[0].entries[app.panels[0].selected].name;
        assert_eq!(selected_name, "target.txt");
        let Some(Overlay::FindFile(state)) = &app.overlay else {
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
        app.overlay = Some(Overlay::FindFile(results_state));

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
        app.overlay = Some(Overlay::FindFile(results_state));

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();
        assert!(matches!(app.mode, Mode::Editing(_)), "sanity");

        crate::editor::handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else {
            panic!("closing the editor should return to Overlay::FindFile, not Mode::Browsing");
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
        app.overlay = Some(Overlay::FindFile(results_state));

        handle_results_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::FindFile(_))));
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
