use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Overlay};
use crate::text_field;

use super::super::background::spawn_search;
use super::super::history;
use super::super::state::{FindFileField, FindFilePhase};

/// Key handling for `FindFilePhase::Typing` -- full-cursor editing
/// (`text_field`) into whichever of the two fields (`query`/
/// `content_query`) `Tab` last selected, `Up`/`Down` to browse that
/// field's own persisted history, and `Enter` to actually run a search
/// (`run_search` below). `Esc` is handled one level up, in
/// `handle_find_file_key`, ahead of this dispatch entirely.
pub(super) fn handle_typing_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Enter {
        return run_search(app);
    }

    let Some(Overlay::FindFile(state)) = &mut app.overlay else {
        unreachable!("handle_find_file_key only dispatches here while Overlay::FindFile(_) is active");
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
    let (field, history_index) = match state.active_field {
        FindFileField::Name => (&mut state.query, &mut state.name_history_index),
        FindFileField::Content => (&mut state.content_query, &mut state.content_history_index),
    };
    if field.apply_key(key) == text_field::EditOutcome::TextChanged {
        *history_index = None; // editing means fresh typing, not still showing a recalled entry
    }
    Ok(())
}

/// `Enter` while typing: spawns `search::search_cancelable` on a
/// background thread (`background::spawn_search`) and switches to
/// `FindFilePhase::Searching` -- doesn't block waiting for it, and
/// doesn't apply any results itself; `background::poll_pending_find_file_search`
/// (driven from `event_loop::wait_for_event`) picks up the finished search
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
    let Some(Overlay::FindFile(state)) = &app.overlay else {
        return Ok(());
    };
    if state.query.is_empty() && state.content_query.is_empty() {
        return Ok(());
    }
    let query = state.query.text().to_string();
    let content_query = state.content_query.text().to_string();
    let root = app.panels[app.active].path.clone();
    debug!(query, content_query, root = %root.display(), "find file: searching");
    history::record_history(&mut app.find_file_name_history, &query);
    history::record_history(&mut app.find_file_content_history, &content_query);
    let pending = spawn_search(root, query, content_query);

    let Some(Overlay::FindFile(state)) = &mut app.overlay else {
        unreachable!("just matched Overlay::FindFile above");
    };
    state.pending = Some(pending);
    state.phase = FindFilePhase::Searching;
    state.export_message = None; // a stale message from a previous search shouldn't linger
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::KeyModifiers;

    use super::super::test_support::{app_with_find_file, wait_for_search};
    use super::*;
    use crate::text_field::TextField;
    use crate::explorer::FindFileState;
    use crate::test_support::key;

    #[test]
    fn typing_inserts_into_the_query() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_typing_key(&mut app, key(KeyCode::Char('a'))).unwrap();
        handle_typing_key(&mut app, key(KeyCode::Char('b'))).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.text(), "ab");
    }

    /// Regression coverage for a real report: neither field supported
    /// text selection at all -- only plain cursor movement and
    /// character-at-a-time editing. `Shift+Left` should open a
    /// selection, matching `explorer::confirm::handle_confirm_transfer_key`'s
    /// own destination field.
    #[test]
    fn shift_left_selects_the_character_before_the_cursor() {
        let mut state = FindFileState::new();
        state.query = TextField::at("abc", 3, None);
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.anchor(), Some(3));
        assert_eq!(state.query.cursor(), 2);
    }

    /// Regression coverage for a real report: `Ctrl+Shift+Left` used to
    /// silently fall into the plain `Shift+Left` (character-wise) arm
    /// instead, since a guard checking `shift` alone doesn't rule out
    /// `ctrl` also being held -- it "didn't work" in the sense of doing
    /// the wrong thing (one character), not nothing at all.
    #[test]
    fn ctrl_shift_left_selects_by_a_whole_word_not_one_character() {
        let mut state = FindFileState::new();
        state.query = TextField::at("one two", 7, None);
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.anchor(), Some(7));
        assert_eq!(state.query.cursor(), 4, "should have jumped back a whole word (\"two\"), not just one character");
    }

    #[test]
    fn ctrl_shift_right_selects_by_a_whole_word() {
        let mut state = FindFileState::new();
        state.query = TextField::at("one two", 0, None);
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.anchor(), Some(0));
        assert_eq!(state.query.cursor(), 3, "should have jumped forward a whole word (\"one\")");
    }

    /// The content field's own Ctrl+Shift selection is independent of
    /// the name field's, same as plain Shift already is.
    #[test]
    fn ctrl_shift_left_on_the_content_field_does_not_touch_the_name_field() {
        let mut state = FindFileState::new();
        state.content_query = TextField::at("one two", 7, None);
        state.active_field = FindFileField::Content;
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.content_query.anchor(), Some(7));
        assert_eq!(state.content_query.cursor(), 4);
        assert_eq!(state.query.anchor(), None, "the name field's own selection shouldn't be touched");
    }

    #[test]
    fn backspace_with_a_selection_deletes_the_whole_selection_not_one_character() {
        let mut state = FindFileState::new();
        state.query = TextField::at("abc", 3, Some(1));
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, key(KeyCode::Backspace)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.text(), "a", "should have removed \"bc\" (the whole selection), not just \"c\"");
        assert_eq!(state.query.anchor(), None);
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut state = FindFileState::new();
        state.query = TextField::at("abc", 3, Some(0));
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.text(), "x");
    }

    /// The content field's own selection is entirely independent of the
    /// name field's -- same "each field owns its own state" convention
    /// this popup already has for cursor position and history.
    #[test]
    fn the_content_fields_selection_is_independent_of_the_name_fields() {
        let mut state = FindFileState::new();
        state.query = TextField::at("name", 4, Some(0));
        state.content_query = TextField::at("content", 7, None);
        state.active_field = FindFileField::Content;
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.content_query.anchor(), Some(7));
        assert_eq!(state.query.anchor(), Some(0), "the name field's own selection shouldn't be touched");
    }

    #[test]
    fn enter_on_an_empty_query_does_not_search() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
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

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.active_field, FindFileField::Content);
        assert_eq!(state.content_query.text(), "x");
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

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.text(), "recent.txt", "Up on the name field should recall the name history, not the content one");

        // Switch to the content field -- Up there should recall its own
        // history, untouched by whatever the name field just did.
        let mut app = app_with_find_file(FindFileState::new());
        app.find_file_content_history = vec!["needle".to_string()];
        handle_typing_key(&mut app, key(KeyCode::Tab)).unwrap();

        handle_typing_key(&mut app, key(KeyCode::Up)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.content_query.text(), "needle");
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

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.query.text(), "recalled.txt!");
        assert_eq!(state.name_history_index, None, "typing should leave history-browsing mode");
    }

    /// Regression coverage for the real request: a search that actually
    /// runs (`Enter`) should record whichever field(s) were used into
    /// their own separate history.
    #[test]
    fn enter_records_both_fields_into_their_own_separate_history() {
        let mut state = FindFileState::new();
        state.query.set_text("*.rs");
        state.content_query.set_text("TODO");
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
        state.content_query.set_text("needle");
        state.active_field = FindFileField::Content;
        let mut app = app_with_find_file(state);
        fs::write(app.panels[0].path.join("a.txt"), b"needle here").unwrap();
        fs::write(app.panels[0].path.join("b.txt"), b"nothing here").unwrap();

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();
        wait_for_search(&mut app);

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results, vec![app.panels[0].path.join("a.txt")]);
    }

    #[test]
    fn enter_on_a_real_query_runs_a_search_and_switches_to_results() {
        let mut state = FindFileState::new();
        state.query = TextField::at("sou", 3, None);
        let mut app = app_with_find_file(state);
        fs::write(app.panels[0].path.join("source.txt"), b"hi").unwrap();

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();
        wait_for_search(&mut app);

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
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
        state.query.set_text("sou");
        let mut app = app_with_find_file(state);

        handle_typing_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::FindFile(state)) = &app.overlay else { panic!("expected Overlay::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Searching);
        assert!(state.pending.is_some());
    }
}
