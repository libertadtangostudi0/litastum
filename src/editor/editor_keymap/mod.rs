use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};
use crate::explorer;

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
        return return_from_editor(app);
    }

    debug!("editor close: unsaved changes, asking to confirm discard");
    let Mode::Editing(editor) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::Editing above");
    };
    app.mode = Mode::ConfirmDiscard(editor);
    Ok(())
}


/// The editor is actually closing for good (no unsaved changes, or they
/// were just discarded) -- reloads the active panel (in case anything
/// changed on disk while editing) and hands control back to wherever
/// `F4` was originally pressed from: `Mode::Browsing` normally,
/// `Mode::FindFile` if it was pressed from the Find file results popup
/// (`app.editor_return_to`, set by `find_file/input.rs::edit_selected_result`),
/// or `Mode::UserMenu` if it was pressed on a `Commands` item in the
/// `F2` user menu (`app.user_menu_command_edit`, set by
/// `explorer::user_menu::input::open_edit_selected_command`) -- each
/// taken (`Option::take`) exactly once here; at most one is ever
/// actually set at a time, since the editor can only have been opened
/// from one place. The user-menu case also has to *finish* the edit
/// (`explorer::finish_command_edit`: read the scratch file's final
/// contents back into the item, persist, delete the scratch file),
/// not just pick which mode to restore -- unlike the other two cases,
/// the editor here was never pointed at the item's real backing file.
/// `Mode::ConfirmDiscard`'s own `Cancel` path (back into the editor,
/// nothing lost) deliberately does *not* call this -- the editor hasn't
/// actually closed there.
fn return_from_editor(app: &mut App) -> Result<()> {
    app.active_panel().reload()?;
    app.mode = if let Some(state) = app.editor_return_to.take() {
        Mode::FindFile(state)
    } else if let Some(edit) = app.user_menu_command_edit.take() {
        Mode::UserMenu(explorer::finish_command_edit(edit))
    } else {
        Mode::Browsing
    };
    Ok(())
}


/// Key handling on the "discard unsaved changes?" prompt: `Y` discards
/// and returns to browsing, `N`/`Esc` cancels back into the editor with
/// nothing lost, anything else is ignored.
pub fn handle_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let command = resolve_confirm_discard(key);
    debug!(?key, ?command, "confirm-discard key");

    match command {
        ConfirmDiscardCommand::Discard => return return_from_editor(app),
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
mod tests;
