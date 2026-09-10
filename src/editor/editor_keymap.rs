use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};

use super::find_history;


/// A user-triggered action while a file is open in the built-in editor,
/// at the level `main.rs` needs to care about. Almost everything —
/// typing, movement, selection, copy/cut/paste — is `edtui`'s own
/// concern once a key reaches `Editor::input`; `Save` (a concept
/// `edtui` has no notion of), `Close` (which `main.rs` must decide
/// whether to honor immediately or forward, depending on whether a
/// selection is active — see `Editor::has_selection`), and `WordSelect`
/// (hand-rolled logic `edtui`'s own declarative keymap can't express —
/// see its own doc comment) are the only things resolved before that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    Close,
    Save,
    /// `Ctrl+Shift+Left`/`Right` -- word-wise selection
    /// (`Editor::extend_word_selection`). Resolved here rather than
    /// left to `edtui`'s own dispatch (`Forward`, below) because no
    /// combination of `bindings.rs`'s declarative `Action` table could
    /// give both "repeated presses keep progressing" and "`Left` undoes
    /// exactly what `Right` just did" -- see
    /// `bindings::extend_word_selection`'s own doc comment for the full
    /// story of why.
    WordSelect { forward: bool },
    /// `Ctrl+A` -- selects the entire buffer (`Editor::select_all`).
    /// Resolved here rather than left to `edtui`'s own dispatch: there's
    /// no entry for it in `bindings.rs`'s declarative table at all (it
    /// was simply never bound), and `edtui`'s own action set has no
    /// single "select everything" primitive to bind to one key input
    /// even if there were -- `Editor::select_all` chains several plain
    /// motions instead, the same shape `WordSelect` above already uses
    /// for logic too involved for one table entry.
    SelectAll,
    /// `Ctrl+F` -- opens the built-in search box (`Editor::start_search`).
    /// Only ever resolved while the box *isn't* already open --
    /// `handle_editor_key` intercepts every key ahead of `resolve`
    /// entirely once `Editor::is_searching()` is true, routing to
    /// `handle_search_key` below instead, so this variant is never
    /// reached a second time to mean "close" or "next match".
    Find,
    /// Not one of the bindings above — forward the raw key event to
    /// `Editor::input`.
    Forward,
    /// A key `edtui` itself has no conversion for at all (see
    /// `edtui_supports_key`'s own doc comment) -- swallowed here rather
    /// than forwarded, so the editor stays isolated from whatever this
    /// key would otherwise mean outside it (a global shortcut on the
    /// browsing screen, or nothing at all).
    Ignore,
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
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Esc => EditorCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => EditorCommand::Save,
        KeyCode::Char('f' | 'F') if ctrl => EditorCommand::Find,
        KeyCode::Char('a' | 'A') if ctrl => EditorCommand::SelectAll,
        KeyCode::Left if ctrl && shift => EditorCommand::WordSelect { forward: false },
        KeyCode::Right if ctrl && shift => EditorCommand::WordSelect { forward: true },
        _ if edtui_supports_key(key.code) => EditorCommand::Forward,
        _ => EditorCommand::Ignore,
    }
}

/// Real crash, reported directly: `F10` (the app's own global quit key
/// on the browsing screen) while a file was open in the editor panicked
/// the whole process with `unimplemented!()` inside `edtui`'s own
/// `KeyCode::from(crossterm::event::KeyCode)` conversion
/// (`edtui-0.11.7/src/events/key/input.rs`, confirmed directly from
/// source) -- forwarded here as `EditorCommand::Forward` like any other
/// unrecognized key, then straight into `Editor::input` ->
/// `EditorEventHandler::on_key_event`, which converts the raw
/// `crossterm::event::KeyEvent` into `edtui`'s own `KeyInput`
/// internally. That conversion only explicitly matches fourteen
/// `crossterm::event::KeyCode` variants (`Char`, `Enter`, `Esc`,
/// `Backspace`, `Delete`, `Tab`, the four arrow keys, `Home`, `End`,
/// `PageUp`, `PageDown`) -- everything else, function keys included,
/// falls through to an unconditional `unimplemented!()` catch-all with
/// no fallback at all, not even a silent no-op.
///
/// The app's own mode-based dispatch (`main.rs::handle_event`) already
/// means a global key like `F10` never reaches the browsing screen's
/// own quit binding while `Mode::Editing` is active -- routing here
/// through `handle_editor_key` is the *only* path a keystroke takes
/// while editing, so "the editor needs isolated key handling" was
/// already true structurally. This crash was really the isolation
/// leaking the *other* way: an unsupported key wasn't being swallowed
/// by the editor, it was being forwarded into a library that has no
/// silent-ignore path for it at all. Matching an explicit allowlist of
/// what `edtui` actually supports (rather than trying to name every
/// unsupported crossterm variant -- function keys, `Insert`, `Null`,
/// `CapsLock`, `Menu`, `KeypadBegin`, `Media(_)`, `Modifier(_)`, and
/// whatever else crossterm might report) means a future `edtui` upgrade
/// that starts supporting more keys just needs this list extended to
/// match, rather than a blocklist that has to keep pace with every
/// crossterm variant that exists.
fn edtui_supports_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Char(_)
            | KeyCode::Enter
            | KeyCode::Esc
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Tab
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
    )
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
/// above) — everything else `edtui` actually understands, including
/// copy/cut/paste/selection, is `edtui`'s own concern once forwarded to
/// `Editor::input`; anything it doesn't (see `edtui_supports_key`'s own
/// doc comment) is silently ignored instead of forwarded, rather than
/// crashing. `Esc` is
/// special-cased further: with an active selection it's forwarded too
/// (so `edtui`'s own binding cancels the selection), only closing the
/// editor once there's nothing selected.
///
/// Moved here from `main.rs` alongside `resolve`/`resolve_confirm_discard`
/// so this module owns editor key handling end to end, the same way
/// `theme_menu.rs`/`menu.rs` each own their own state and handling —
/// `main.rs` stays a thin dispatcher over `Mode`.
pub fn handle_editor_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if matches!(&app.mode, Mode::Editing(editor) if editor.is_searching()) {
        return handle_search_key(app, key);
    }

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
        EditorCommand::Find => active_editor.start_search(),
        EditorCommand::SelectAll => active_editor.select_all(),
        EditorCommand::WordSelect { forward } => active_editor.extend_word_selection(forward),
        EditorCommand::Forward => active_editor.input(key),
        EditorCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling while the `Ctrl+F` search box is open -- intercepted
/// ahead of `resolve`/the normal table entirely (see `handle_editor_key`
/// above), the same way an in-progress word-select drag or the discard
/// prompt each own their own key handling rather than sharing the
/// ordinary editor dispatch. Typing filters live (`Editor::search_push_char`
/// re-runs `edtui`'s own search on every keystroke); `Enter`/`Shift+Enter`
/// jump to the next/previous match, VS Code's own `Ctrl+F` convention --
/// `Up`/`Down` were tried for this first and reported wrong: those are
/// for browsing *history* instead (`Editor::search_history_up`/`_down`),
/// the same way a shell's own `Up`/`Down` recall past commands rather
/// than doing anything to the command currently being typed. `End`
/// accepts the ghost-text history suggestion shown after the query, if
/// any (`find_history::suggest`); `Esc` closes the box and records the
/// query into the persisted search history
/// (`find_history::record_history`/`save_history`) if it isn't empty.
fn handle_search_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::Editing(active_editor) = &mut app.mode else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => {
            let query = active_editor.search_query();
            active_editor.stop_search();
            if !query.is_empty() {
                find_history::record_history(&mut app.search_history, &query);
            }
            // Deliberately doesn't save to disk here -- this function is
            // heavily unit-tested (see `handle_search_key_tests` below),
            // and saving here would mean every one of those tests writes
            // a real `editor_search_history.txt` into the cwd, exactly
            // the trap `command_line::history` avoids by keeping
            // `record_history` (memory) and `save_history` (disk)
            // separate, with only the latter's *own* caller
            // (`browsing::run_command_line`) touching disk -- see that
            // function's own doc comment. `main.rs::main` persists
            // `app.search_history` once at clean exit instead, the same
            // in-memory-during-the-session shape without any unit-tested
            // code path ever touching the real filesystem.
        }
        KeyCode::Up => active_editor.search_history_up(&app.search_history),
        KeyCode::Down => active_editor.search_history_down(&app.search_history),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => active_editor.search_previous(),
        KeyCode::Enter => active_editor.search_next(),
        KeyCode::Backspace => active_editor.search_pop_char(),
        KeyCode::End => {
            let query = active_editor.search_query();
            if let Some(suggestion) = find_history::suggest(&app.search_history, &query) {
                let suggestion = suggestion.to_string();
                active_editor.accept_search_suggestion(&suggestion);
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => active_editor.search_push_char(c),
        _ => {}
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
    fn ctrl_f_resolves_to_find() {
        assert_eq!(resolve(ctrl_key('f')), EditorCommand::Find);
        assert_eq!(resolve(ctrl_key('F')), EditorCommand::Find, "should match uppercase too, same reasoning as Ctrl+S");
    }

    #[test]
    fn plain_f_without_ctrl_is_forwarded_not_find() {
        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Forward);
    }

    #[test]
    fn ctrl_a_resolves_to_select_all() {
        assert_eq!(resolve(ctrl_key('a')), EditorCommand::SelectAll);
        assert_eq!(resolve(ctrl_key('A')), EditorCommand::SelectAll, "should match uppercase too, same reasoning as Ctrl+S");
    }

    #[test]
    fn plain_a_without_ctrl_is_forwarded_not_select_all() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Forward);
    }

    #[test]
    fn ctrl_c_is_forwarded_to_edtui_not_handled_here() {
        // Copy/cut/paste are edtui's own concern now (see its custom
        // keymap in editor.rs) -- this module no longer special-cases them.
        assert_eq!(resolve(ctrl_key('c')), EditorCommand::Forward);
    }

    #[test]
    fn ctrl_shift_right_resolves_to_word_select_forward() {
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(resolve(key), EditorCommand::WordSelect { forward: true });
    }

    #[test]
    fn ctrl_shift_left_resolves_to_word_select_backward() {
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(resolve(key), EditorCommand::WordSelect { forward: false });
    }

    #[test]
    fn plain_ctrl_right_without_shift_is_forwarded_to_edtui() {
        // Plain Ctrl+Right (no selection) is still `bindings.rs`'s own
        // declarative-table concern -- only the Shift combination is
        // special-cased here.
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
        assert_eq!(resolve(key), EditorCommand::Forward);
    }

    #[test]
    fn plain_shift_right_without_ctrl_is_forwarded_to_edtui() {
        // Character-wise Shift+Right stays edtui's own table entry too.
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(resolve(key), EditorCommand::Forward);
    }

    /// Regression test for the real crash: `F10` (the app's own global
    /// quit key on the browsing screen) has no conversion in `edtui`'s
    /// own `KeyCode::from` at all -- forwarding it panicked the whole
    /// process. Must resolve to `Ignore`, not `Forward`.
    #[test]
    fn f10_is_ignored_not_forwarded() {
        let key = KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Ignore);
    }

    /// Every function key shares the same gap in `edtui`'s own
    /// conversion, not just `F10` -- pinned down as a range rather than
    /// one magic number.
    #[test]
    fn every_function_key_is_ignored_not_forwarded() {
        for n in 1..=12 {
            let key = KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE);
            assert_eq!(resolve(key), EditorCommand::Ignore, "F{n} should be ignored, not forwarded to edtui");
        }
    }

    /// `Insert` is a real crossterm `KeyCode` variant `edtui`'s own
    /// conversion also has no arm for -- confirms this isn't
    /// function-keys-only special-casing.
    #[test]
    fn insert_key_is_ignored_not_forwarded() {
        let key = KeyEvent::new(KeyCode::Insert, KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Ignore);
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

    /// Regression test for the real, reported crash: `cargo run` ->
    /// open a file -> `F4` -> `F10` panicked the whole process
    /// (`edtui`'s own `KeyCode::from` conversion has no arm for `F10`
    /// at all). Must stay open, in `Mode::Editing`, completely
    /// unaffected -- the editor's key handling is isolated from
    /// whatever `F10` means on the browsing screen (global quit),
    /// exactly as requested.
    #[test]
    fn handle_editor_key_f10_does_not_crash_or_close_the_editor() {
        let (mut app, _path) = open_editor_app("hi\n");

        handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE)).unwrap();

        assert!(matches!(app.mode, Mode::Editing(_)), "F10 must not close the editor or crash while editing");
    }

    /// Regression coverage for the real request: `Ctrl+A` should select
    /// the whole buffer -- verified functionally (deleting the
    /// selection clears everything) rather than asserting on exact
    /// cursor coordinates, which would be tied to `edtui`'s own
    /// row/column indexing details.
    #[test]
    fn handle_editor_key_ctrl_a_selects_the_entire_buffer() {
        let (mut app, path) = open_editor_app("hello\nworld\n");

        handle_editor_key(&mut app, ctrl_key('a')).unwrap();
        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert!(editor.has_selection(), "Ctrl+A should open a selection");

        handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();
        let Mode::Editing(active_editor) = &mut app.mode else { unreachable!() };
        active_editor.save().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "", "deleting a Ctrl+A selection should clear the whole buffer");
    }

    /// Regression test for the real, reported bug: `Ctrl+A` then
    /// `Backspace` then a single `Ctrl+Z` did nothing at all -- only a
    /// *second* `Ctrl+Z` actually restored the deleted text. Root cause:
    /// the old table entry captured an undo checkpoint twice for one
    /// keypress (once correctly, inside `DeleteSelection`, and once more
    /// spuriously when returning to `Insert` mode afterward) -- see
    /// `bindings::is_selection_consuming_key`'s own doc comment for the
    /// full mechanism. One `Ctrl+Z` must restore everything now.
    #[test]
    fn handle_editor_key_ctrl_z_undoes_a_select_all_delete_in_one_press() {
        let (mut app, path) = open_editor_app("hello\nworld\n");
        handle_editor_key(&mut app, ctrl_key('a')).unwrap();
        handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();

        handle_editor_key(&mut app, ctrl_key('z')).unwrap();

        let Mode::Editing(active_editor) = &mut app.mode else { unreachable!() };
        active_editor.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n", "a single Ctrl+Z should restore everything the Ctrl+A/Backspace deleted");
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

    mod handle_search_key_tests {
        use super::*;

        #[test]
        fn ctrl_f_opens_the_search_box() {
            let (mut app, _path) = open_editor_app("hello world\n");

            handle_editor_key(&mut app, ctrl_key('f')).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert!(editor.is_searching());
        }

        #[test]
        fn typing_filters_the_query_live_and_jumps_to_the_first_match() {
            let (mut app, _path) = open_editor_app("hello world\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();

            for c in "world".chars() {
                handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
            }

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "world");
            assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 6 }, "cursor should jump to \"world\"'s own start");
        }

        #[test]
        fn backspace_removes_the_last_query_character() {
            let (mut app, _path) = open_editor_app("hello world\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            handle_editor_key(&mut app, key(KeyCode::Char('w'))).unwrap();
            handle_editor_key(&mut app, key(KeyCode::Char('o'))).unwrap();

            handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "w");
        }

        /// Real requirement, stated directly: navigation is plain `Up`/
        /// `Down`, not `F3`/`Shift+F3` -- there's no bare-arrow conflict
        /// to work around here the way the always-live command line has,
        /// since this is its own popup.
        #[test]
        fn enter_and_shift_enter_navigate_between_matches() {
            let (mut app, _path) = open_editor_app("cat dog cat\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            for c in "cat".chars() {
                handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
            }
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 }, "sanity: should start on the first \"cat\"");

            handle_editor_key(&mut app, key(KeyCode::Enter)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 8 }, "Enter should jump to the second \"cat\"");

            handle_editor_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 }, "Shift+Enter should jump back to the first \"cat\"");
        }

        /// Real requirement, stated directly after `Up`/`Down` was
        /// first tried for match navigation and reported wrong: those
        /// keys browse the *search history* instead, a shell-`Up`-arrow
        /// convention -- first press recalls the most recent past
        /// query, further presses step further back, `Down` steps back
        /// toward the present and clears the box once past the newest
        /// entry.
        #[test]
        fn up_and_down_browse_search_history_not_matches() {
            let (mut app, _path) = open_editor_app("cat dog cat\n");
            app.search_history = vec!["dog".to_string(), "cat".to_string()];
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();

            handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "cat", "first Up should recall the most recent past query");

            handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "dog", "second Up should step further back");

            handle_editor_key(&mut app, key(KeyCode::Down)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "cat", "Down should step back toward the most recent entry");

            handle_editor_key(&mut app, key(KeyCode::Down)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "", "Down past the newest entry should clear the box");
        }

        #[test]
        fn typing_after_browsing_history_resets_it() {
            let (mut app, _path) = open_editor_app("hello world\n");
            app.search_history = vec!["hello".to_string()];
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "hello", "sanity: history recalled");

            handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "hello!");

            // A further Up should start fresh from the most recent
            // entry again, not continue on from wherever browsing left
            // off before the edit.
            handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "hello");
        }

        #[test]
        fn esc_leaves_the_cursor_right_after_the_found_match() {
            let (mut app, _path) = open_editor_app("hello world\n");
            let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
            editor.input(key(KeyCode::Right));
            editor.input(key(KeyCode::Right)); // cursor now at column 2, before opening search
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            for c in "world".chars() {
                handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
            }

            handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert!(!editor.is_searching(), "should have closed the box");
            assert_eq!(
                editor.cursor().col,
                11,
                "should land right after \"world\"'s own last letter ('d', column 10) -- not on it, and not revert to where search started"
            );
        }

        /// Regression test for the real report: searching "lso" inside
        /// "also" left the cursor visually *between* 's' and the final
        /// 'o' instead of after it -- `stop_search` was landing directly
        /// *on* the match's own last character, which only reads
        /// correctly while a selection is active (`cursor_screen_position`'s
        /// own +1 rendering shift, which doesn't fire here since closing
        /// the search box never sets `state.selection`).
        #[test]
        fn esc_lands_after_the_match_not_visually_one_short_of_it() {
            let (mut app, _path) = open_editor_app("also\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            for c in "lso".chars() {
                handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
            }

            handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.cursor().col, 4, "should be right after the final 'o' (column 3), not on it");
        }

        /// The revert-to-where-search-started behavior still applies
        /// when nothing was actually found -- there's no match to leave
        /// the cursor on.
        #[test]
        fn esc_with_no_match_found_reverts_to_where_search_started() {
            let (mut app, _path) = open_editor_app("hello world\n");
            let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
            editor.input(key(KeyCode::Right));
            editor.input(key(KeyCode::Right)); // cursor now at column 2
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            for c in "xyz".chars() {
                handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
            }

            handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.cursor().col, 2, "nothing was found -- should revert to where search started");
        }

        /// Real requirement, stated directly: a separate search-history
        /// file, recorded the same way `command_line::history` is.
        #[test]
        fn esc_records_a_non_empty_query_into_search_history() {
            let (mut app, _path) = open_editor_app("hello world\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            for c in "world".chars() {
                handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
            }

            handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

            assert_eq!(app.search_history, vec!["world"]);
        }

        #[test]
        fn esc_with_an_empty_query_records_nothing() {
            let (mut app, _path) = open_editor_app("hello world\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();

            handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

            assert!(app.search_history.is_empty());
        }

        /// Real requirement, stated directly: the query field should
        /// offer history-based suggestions "similar to the command
        /// line" -- `End` accepts the ghost-text suggestion shown after
        /// the typed query (`find_history::suggest`, rendered by
        /// `ui::editor_find::draw_find_popup`).
        #[test]
        fn end_accepts_the_history_suggestion() {
            let (mut app, _path) = open_editor_app("hello world\n");
            app.search_history = vec!["world".to_string()];
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();
            handle_editor_key(&mut app, key(KeyCode::Char('w'))).unwrap();

            handle_editor_key(&mut app, key(KeyCode::End)).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert_eq!(editor.search_query(), "world");
        }

        #[test]
        fn plain_keys_are_swallowed_by_the_search_box_not_forwarded_to_the_buffer() {
            let (mut app, _path) = open_editor_app("hello world\n");
            handle_editor_key(&mut app, ctrl_key('f')).unwrap();

            handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap();

            let Mode::Editing(editor) = &app.mode else { unreachable!() };
            assert!(!editor.is_dirty(), "typing into the search box must not edit the buffer");
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
