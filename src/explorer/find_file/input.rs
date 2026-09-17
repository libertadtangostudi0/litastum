use std::path::Path;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::text_field;

use super::background::spawn_search;
use super::export::export_results;
use super::state::{FindFileField, FindFilePhase};

/// Key handling for all three phases of the popup: typing the query
/// (full-cursor editing, `text_field.rs` — same reasoning as the F5/F6
/// transfer prompt, this is a modal popup with no panel navigation
/// happening under it), a search actually running in the background
/// (`Searching`, see `background.rs`), and, once it finishes, picking a
/// result with `Up`/`Down`/`Enter`/`Tab`/`F4`. `Esc` closes from any
/// phase -- during `Searching`, it also cancels the background search
/// first (`PendingSearch::cancel`), so the thread stops promptly
/// instead of continuing to churn on a search nothing's listening to
/// the result of anymore.
pub fn handle_find_file_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        if let Mode::FindFile(state) = &app.mode {
            if let Some(pending) = &state.pending {
                pending.cancel();
            }
        }
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
            if key.code == KeyCode::Tab {
                state.active_field = match state.active_field {
                    FindFileField::Name => FindFileField::Content,
                    FindFileField::Content => FindFileField::Name,
                };
                return Ok(());
            }
            let (field, cursor) = match state.active_field {
                FindFileField::Name => (&mut state.query, &mut state.cursor),
                FindFileField::Content => (&mut state.content_query, &mut state.content_cursor),
            };
            match key.code {
                KeyCode::Backspace => text_field::backspace(field, cursor),
                KeyCode::Left => text_field::move_left(cursor),
                KeyCode::Right => text_field::move_right(field, cursor),
                KeyCode::Home => text_field::move_home(cursor),
                KeyCode::End => text_field::move_end(field, cursor),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text_field::insert_char(field, cursor, c);
                }
                _ => {}
            }
        }
        // Nothing bound here besides `Esc` (handled above, ahead of
        // this match, since it also needs to cancel the background
        // search) -- this is a passive "please wait" screen, not
        // something to navigate.
        FindFilePhase::Searching => {}
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

/// `Enter` while typing: spawns `search::search_cancelable` on a
/// background thread (`background::spawn_search`) and switches to
/// `FindFilePhase::Searching` -- doesn't block waiting for it, and
/// doesn't apply any results itself; `background::poll_pending_find_file_search`
/// (driven from `main.rs::wait_for_event`) picks up the finished search
/// and switches to `FindFilePhase::Results` once it's actually done. A
/// no-op only if *both* fields are empty (nothing sensible to search
/// for) -- either one alone is enough, matching Far Manager's own
/// two-field dialog (a bare "Text to find", with the name mask left as
/// its own implicit "match everything", is a legitimate search there
/// too).
fn run_search(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    if state.query.is_empty() && state.content_query.is_empty() {
        return Ok(());
    }
    let query = state.query.clone();
    let content_query = state.content_query.clone();
    let root = app.panels[app.active].path.clone();
    debug!(query, content_query, root = %root.display(), "find file: searching");
    let pending = spawn_search(root, query, content_query);

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("just matched Mode::FindFile above");
    };
    state.pending = Some(pending);
    state.phase = FindFilePhase::Searching;
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

    use super::*;
    use crate::explorer::FindFileState;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_find_file(state: FindFileState) -> App {
        let mut app = test_app(unique_scratch_dir("find-file-app"));
        app.mode = Mode::FindFile(state);
        app
    }

    /// Polls `Mode::FindFile`'s pending background search
    /// (`background::poll_pending_find_file_search`) until it leaves
    /// `FindFilePhase::Searching` -- `run_search` (triggered by `Enter`)
    /// only *starts* a search now, on a real background thread, rather
    /// than blocking until it's done the way the old synchronous
    /// version did; tests that care about the actual results need to
    /// wait for it the same way `main.rs::wait_for_event` does in the
    /// real app.
    fn wait_for_search(app: &mut App) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let Mode::FindFile(state) = &app.mode else {
                panic!("expected Mode::FindFile while waiting for a search");
            };
            if state.phase != FindFilePhase::Searching {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "search did not finish within the test timeout");
            crate::explorer::poll_pending_find_file_search(app);
        }
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

    /// Regression coverage: `Tab` while typing switches which field
    /// further typed characters and edits reach, rather than falling
    /// through to some other binding (nothing else claims `Tab` during
    /// `Typing`).
    #[test]
    fn tab_switches_the_active_field_and_typing_follows_it() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_find_file_key(&mut app, key(KeyCode::Tab)).unwrap();
        handle_find_file_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.active_field, FindFileField::Content);
        assert_eq!(state.content_query, "x");
        assert!(state.query.is_empty(), "typing after Tab should not still reach the name field");
    }

    /// A bare "Text to find" with an empty name mask is still a
    /// legitimate search -- Far Manager's own two-field dialog treats
    /// an empty mask as "match every name."
    #[test]
    fn enter_with_only_a_content_query_still_searches() {
        let mut state = FindFileState::new();
        state.content_query = "needle".to_string();
        state.active_field = FindFileField::Content;
        let mut app = app_with_find_file(state);
        fs::write(app.panels[0].path.join("a.txt"), b"needle here").unwrap();
        fs::write(app.panels[0].path.join("b.txt"), b"nothing here").unwrap();

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();
        wait_for_search(&mut app);

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results, vec![app.panels[0].path.join("a.txt")]);
    }

    #[test]
    fn enter_on_a_real_query_runs_a_search_and_switches_to_results() {
        let mut state = FindFileState::new();
        state.query = "sou".to_string();
        state.cursor = 3;
        let mut app = app_with_find_file(state);
        fs::write(app.panels[0].path.join("source.txt"), b"hi").unwrap();

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();
        wait_for_search(&mut app);

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results, vec![app.panels[0].path.join("source.txt")]);
    }

    /// Immediately after `Enter`, before the background search has had
    /// a chance to finish, the popup should be showing
    /// `FindFilePhase::Searching`, not still `Typing` and not already
    /// `Results` -- confirms `run_search` itself never blocks.
    #[test]
    fn enter_on_a_real_query_switches_to_searching_before_the_background_thread_finishes() {
        let mut state = FindFileState::new();
        state.query = "sou".to_string();
        let mut app = app_with_find_file(state);

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Searching);
        assert!(state.pending.is_some());
    }

    /// `Esc` during `FindFilePhase::Searching` should cancel the
    /// background search (not just close the popup) -- confirmed
    /// indirectly, since there's no synchronous way to observe the
    /// background thread noticing: the search is spawned over a large
    /// enough tree that, if cancellation weren't actually wired up, it
    /// would still be running (and would eventually try to send its
    /// full results into a channel this test's own `app`/`state` no
    /// longer owns, silently, per `PendingSearch`'s own doc comment).
    #[test]
    fn esc_during_searching_cancels_the_background_search() {
        let mut state = FindFileState::new();
        state.query = "file".to_string();
        let mut app = app_with_find_file(state);
        for i in 0..500 {
            fs::write(app.panels[0].path.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing), "Esc should still close the popup, same as every other phase");
    }

    #[test]
    fn esc_closes_from_any_phase() {
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

        handle_find_file_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();
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

        handle_find_file_key(&mut app, KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.mode, Mode::FindFile(_)));
    }
}
