use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};
use crate::text_field;

use super::super::background::spawn_search;
use super::super::history;
use super::super::state::{FindFileField, FindFilePhase};

/// Key handling for `FindFilePhase::Typing` -- full-cursor editing
/// (`text_field.rs`) into whichever of the two fields (`query`/
/// `content_query`) `Tab` last selected, `Up`/`Down` to browse that
/// field's own persisted history, and `Enter` to actually run a search
/// (`run_search` below). `Esc` is handled one level up, in
/// `handle_find_file_key`, ahead of this dispatch entirely.
pub(super) fn handle_typing_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Enter {
        return run_search(app);
    }

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("handle_find_file_key only dispatches here while Mode::FindFile(_) is active");
    };
    if key.code == KeyCode::Tab {
        state.active_field = match state.active_field {
            FindFileField::Name => FindFileField::Content,
            FindFileField::Content => FindFileField::Name,
        };
        return Ok(());
    }
    // `Up`/`Down` browse the *active* field's own persisted history (a
    // shell's own `Up`-arrow convention, same shape
    // `Editor::search_history_up`/`_down` already use for the built-in
    // editor's `Ctrl+F` box) -- requested directly, so each field keeps
    // its own separate history rather than one shared list. Unbound
    // anywhere else during `Typing`, so this is a pure addition, no
    // existing binding to conflict with.
    if key.code == KeyCode::Up {
        match state.active_field {
            FindFileField::Name => state.name_history_up(&app.find_file_name_history),
            FindFileField::Content => state.content_history_up(&app.find_file_content_history),
        }
        return Ok(());
    }
    if key.code == KeyCode::Down {
        match state.active_field {
            FindFileField::Name => state.name_history_down(&app.find_file_name_history),
            FindFileField::Content => state.content_history_down(&app.find_file_content_history),
        }
        return Ok(());
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let (field, cursor, selection_anchor, history_index) = match state.active_field {
        FindFileField::Name => (&mut state.query, &mut state.cursor, &mut state.selection_anchor, &mut state.name_history_index),
        FindFileField::Content => (&mut state.content_query, &mut state.content_cursor, &mut state.content_selection_anchor, &mut state.content_history_index),
    };
    match key.code {
        // Backspace/Delete remove the active selection instead of one
        // character, if there is one -- `text_field::delete_selection`
        // reports whether it did anything, so the single-character path
        // only runs when there wasn't a selection to consume instead.
        // Same shape as `explorer::confirm::handle_confirm_transfer_key`'s
        // own destination field, reported missing here directly.
        KeyCode::Backspace => {
            let removed_selection = text_field::delete_selection(field, cursor, selection_anchor);
            if !removed_selection {
                text_field::backspace(field, cursor);
            }
            *history_index = None; // editing means fresh typing, not still showing a recalled entry
        }
        KeyCode::Delete => {
            let removed_selection = text_field::delete_selection(field, cursor, selection_anchor);
            if !removed_selection {
                text_field::delete_forward(field, cursor);
            }
            *history_index = None;
        }
        // Shift+Left/Right (selection) is checked ahead of Ctrl+Left/
        // Right and plain Left/Right below -- `KeyCode::Left` alone
        // can't distinguish "extend selection" from "move" or "jump a
        // word".
        KeyCode::Left if shift => text_field::extend_selection_left(cursor, selection_anchor),
        KeyCode::Right if shift => text_field::extend_selection_right(field, cursor, selection_anchor),
        KeyCode::Left if ctrl => {
            *selection_anchor = None;
            text_field::move_word_left(field, cursor);
        }
        KeyCode::Right if ctrl => {
            *selection_anchor = None;
            text_field::move_word_right(field, cursor);
        }
        // Plain Left/Right with a selection active collapses to that
        // selection's near edge (standard editor behavior) rather than
        // moving one further character past it.
        KeyCode::Left => text_field::collapse_selection_left(cursor, selection_anchor),
        KeyCode::Right => text_field::collapse_selection_right(field, cursor, selection_anchor),
        KeyCode::Home => {
            *selection_anchor = None;
            text_field::move_home(cursor);
        }
        KeyCode::End => {
            *selection_anchor = None;
            text_field::move_end(field, cursor);
        }
        // Typing over an active selection replaces it, like any normal
        // text field -- delete it first, then insert at the (now
        // collapsed) cursor.
        KeyCode::Char(c) if !ctrl => {
            text_field::delete_selection(field, cursor, selection_anchor);
            text_field::insert_char(field, cursor, c);
            *history_index = None; // same reasoning as Backspace above
        }
        _ => {}
    }
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
///
/// Also records whichever field(s) are non-empty into their own
/// persisted history (`history::record_history`) -- `Enter` is this
/// popup's actual "submit" moment (unlike the editor's `Ctrl+F` box,
/// which has no separate run step and records on `Esc` instead), so
/// this is the closest analogue to the command line's own "record on
/// run." In-memory only here, same reasoning as the editor/command-line
/// history modules' own split between `record_history` (memory) and
/// `save_history` (disk) -- `main.rs::main` persists both files once at
/// clean exit, keeping this function's own extensive unit tests
/// filesystem-free.
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
    history::record_history(&mut app.find_file_name_history, &query);
    history::record_history(&mut app.find_file_content_history, &content_query);
    let pending = spawn_search(root, query, content_query);

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("just matched Mode::FindFile above");
    };
    state.pending = Some(pending);
    state.phase = FindFilePhase::Searching;
    state.export_message = None; // a stale message from a previous search shouldn't linger
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::test_support::{app_with_find_file, wait_for_search};
    use super::*;
    use crate::explorer::FindFileState;
    use crate::test_support::key;

    #[test]
    fn typing_inserts_into_the_query() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_typing_key(&mut app, key(KeyCode::Char('a'))).unwrap();
        handle_typing_key(&mut app, key(KeyCode::Char('b'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "ab");
    }

    /// Regression coverage for a real report: neither field supported
    /// text selection at all -- only plain cursor movement and
    /// character-at-a-time editing. `Shift+Left` should open a
    /// selection, matching `explorer::confirm::handle_confirm_transfer_key`'s
    /// own destination field.
    #[test]
    fn shift_left_selects_the_character_before_the_cursor() {
        let mut state = FindFileState::new();
        state.query = "abc".to_string();
        state.cursor = 3;
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.selection_anchor, Some(3));
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn backspace_with_a_selection_deletes_the_whole_selection_not_one_character() {
        let mut state = FindFileState::new();
        state.query = "abc".to_string();
        state.cursor = 3;
        state.selection_anchor = Some(1);
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, key(KeyCode::Backspace)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "a", "should have removed \"bc\" (the whole selection), not just \"c\"");
        assert_eq!(state.selection_anchor, None);
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut state = FindFileState::new();
        state.query = "abc".to_string();
        state.cursor = 3;
        state.selection_anchor = Some(0);
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "x");
    }

    /// The content field's own selection is entirely independent of the
    /// name field's -- same "each field owns its own state" convention
    /// this popup already has for cursor position and history.
    #[test]
    fn the_content_fields_selection_is_independent_of_the_name_fields() {
        let mut state = FindFileState::new();
        state.query = "name".to_string();
        state.cursor = 4;
        state.selection_anchor = Some(0);
        state.content_query = "content".to_string();
        state.content_cursor = 7;
        state.active_field = FindFileField::Content;
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.content_selection_anchor, Some(7));
        assert_eq!(state.selection_anchor, Some(0), "the name field's own selection shouldn't be touched");
    }

    #[test]
    fn enter_on_an_empty_query_does_not_search() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();

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

        handle_typing_key(&mut app, key(KeyCode::Tab)).unwrap();
        handle_typing_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.active_field, FindFileField::Content);
        assert_eq!(state.content_query, "x");
        assert!(state.query.is_empty(), "typing after Tab should not still reach the name field");
    }

    /// Regression coverage for the real request: each field browses its
    /// *own* persisted history with `Up`/`Down`, independently of the
    /// other field.
    #[test]
    fn up_recalls_the_active_fields_own_history() {
        let mut app = app_with_find_file(FindFileState::new());
        app.find_file_name_history = vec!["old.txt".to_string(), "recent.txt".to_string()];
        app.find_file_content_history = vec!["needle".to_string()];

        handle_typing_key(&mut app, key(KeyCode::Up)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "recent.txt", "Up on the name field should recall the name history, not the content one");

        // Switch to the content field -- Up there should recall its own
        // history, untouched by whatever the name field just did.
        let mut app = app_with_find_file(FindFileState::new());
        app.find_file_content_history = vec!["needle".to_string()];
        handle_typing_key(&mut app, key(KeyCode::Tab)).unwrap();

        handle_typing_key(&mut app, key(KeyCode::Up)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.content_query, "needle");
    }

    /// Typing after recalling a history entry with `Up` should leave
    /// fresh typing in place, not silently keep browsing history from
    /// wherever `Up` last left it.
    #[test]
    fn typing_after_up_leaves_history_browsing() {
        let mut app = app_with_find_file(FindFileState::new());
        app.find_file_name_history = vec!["recalled.txt".to_string()];

        handle_typing_key(&mut app, key(KeyCode::Up)).unwrap();
        handle_typing_key(&mut app, key(KeyCode::Char('!'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "recalled.txt!");
        assert_eq!(state.name_history_index, None, "typing should leave history-browsing mode");
    }

    /// Regression coverage for the real request: a search that actually
    /// runs (`Enter`) should record whichever field(s) were used into
    /// their own separate history.
    #[test]
    fn enter_records_both_fields_into_their_own_separate_history() {
        let mut state = FindFileState::new();
        state.query = "*.rs".to_string();
        state.content_query = "TODO".to_string();
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();
        wait_for_search(&mut app);

        assert_eq!(app.find_file_name_history, vec!["*.rs"]);
        assert_eq!(app.find_file_content_history, vec!["TODO"]);
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

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();
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

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();
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

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Searching);
        assert!(state.pending.is_some());
    }
}
