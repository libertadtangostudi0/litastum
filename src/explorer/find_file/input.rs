use std::path::Path;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::text_field;

use super::export::export_results;
use super::search::search;
use super::state::FindFilePhase;

/// Key handling for both phases of the popup: typing the query
/// (full-cursor editing, `text_field.rs` — same reasoning as the F5/F6
/// transfer prompt, this is a modal popup with no panel navigation
/// happening under it) and, once `Enter` runs a search, picking a
/// result with `Up`/`Down`/`Enter`/`Tab`/`F4`. `Esc` closes from either
/// phase.
pub fn handle_find_file_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        app.mode = Mode::Browsing;
        return Ok(());
    }

    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };

    match state.phase {
        FindFilePhase::Typing => {
            if key.code == KeyCode::Enter {
                return run_search(app);
            }

            let Mode::FindFile(state) = &mut app.mode else {
                unreachable!("just matched Mode::FindFile above");
            };
            match key.code {
                KeyCode::Backspace => text_field::backspace(&mut state.query, &mut state.cursor),
                KeyCode::Left => text_field::move_left(&mut state.cursor),
                KeyCode::Right => text_field::move_right(&state.query, &mut state.cursor),
                KeyCode::Home => text_field::move_home(&mut state.cursor),
                KeyCode::End => text_field::move_end(&state.query, &mut state.cursor),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text_field::insert_char(&mut state.query, &mut state.cursor, c);
                }
                _ => {}
            }
        }
        FindFilePhase::Results => match key.code {
            KeyCode::Up => {
                let Mode::FindFile(state) = &mut app.mode else {
                    unreachable!("just matched Mode::FindFile above");
                };
                state.selected = state.selected.saturating_sub(1);
            }
            KeyCode::Down => {
                let Mode::FindFile(state) = &mut app.mode else {
                    unreachable!("just matched Mode::FindFile above");
                };
                if state.selected + 1 < state.results.len() {
                    state.selected += 1;
                }
            }
            KeyCode::Enter => return open_selected_result(app),
            KeyCode::Tab => return goto_selected_result_directory(app),
            KeyCode::F(4) => return edit_selected_result(app),
            KeyCode::Char('s' | 'S') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return run_export(app);
            }
            _ => {}
        },
    }

    Ok(())
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

/// `Enter` while typing: runs `search` from the active panel's
/// directory and switches to `FindFilePhase::Results`. A no-op on an
/// empty query (nothing sensible to search for).
fn run_search(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    if state.query.is_empty() {
        return Ok(());
    }
    let query = state.query.clone();
    let root = app.panels[app.active].path.clone();
    debug!(query, root = %root.display(), "find file: searching");
    let results = search(&root, &query);
    debug!(count = results.len(), "find file: search finished");

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("just matched Mode::FindFile above");
    };
    state.results = results;
    state.selected = 0;
    state.phase = FindFilePhase::Results;
    state.export_message = None; // a stale message from a previous search shouldn't linger
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
/// same as that function. Closes the popup on success, same as `Enter`
/// -- there's nothing left to do with the results list once the editor
/// is open over it.
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
    if let Ok(editor) = Editor::open(path, syntax_theme) {
        app.mode = Mode::Editing(editor);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::explorer::FindFileState;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_find_file(state: FindFileState) -> App {
        let mut app = test_app(unique_scratch_dir("find-file-app"));
        app.mode = Mode::FindFile(state);
        app
    }

    #[test]
    fn typing_inserts_into_the_query() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_find_file_key(&mut app, key(KeyCode::Char('a'))).unwrap();
        handle_find_file_key(&mut app, key(KeyCode::Char('b'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "ab");
    }

    #[test]
    fn enter_on_an_empty_query_does_not_search() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Typing);
    }

    #[test]
    fn enter_on_a_real_query_runs_a_search_and_switches_to_results() {
        let mut state = FindFileState::new();
        state.query = "sou".to_string();
        state.cursor = 3;
        let mut app = app_with_find_file(state);
        fs::write(app.panels[0].path.join("source.txt"), b"hi").unwrap();

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results, vec![app.panels[0].path.join("source.txt")]);
    }

    #[test]
    fn esc_closes_from_either_phase() {
        let mut app = app_with_find_file(FindFileState::new());
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing));

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        let mut app = app_with_find_file(results_state);
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing));
    }

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

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

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

        handle_find_file_key(&mut app, key(KeyCode::Down)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.selected, 1);
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

        handle_find_file_key(&mut app, key(KeyCode::Tab)).unwrap();

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

        handle_find_file_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.mode, Mode::Editing(_)));
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

        handle_find_file_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.mode, Mode::FindFile(_)));
    }
}
