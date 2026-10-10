use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::path_edit::{PathEdit, PathEditKey};
use crate::explorer;
use crate::notice::Notice;
use crate::yes_no::{self, Answer};

use super::find_history;
use super::keymap_mode::EditorKeymapMode;
use super::Editor;


/// An editor key resolved before (or instead of) `Editor::input`: things
/// `edtui` has no notion of (save, the search box, the menu) or that its
/// declarative table can't express. Everything else is `Forward`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    Close,
    Save,
    /// `Shift+F2` -- the title becomes a path field to save as (Far).
    SaveAs,
    /// `Ctrl+Shift+Left`/`Right` -- word-wise selection
    /// (`Editor::extend_word_selection`), hand-rolled because no table
    /// entry could express it; `Standard` only. History:
    /// docs/history/word-select.md.
    WordSelect { forward: bool },
    /// `Ctrl+Down`/`Ctrl+Up` -- to the next/previous block of code
    /// (`Editor::move_by_block`); `Standard` only.
    BlockMove { forward: bool },
    /// `Ctrl+A` -- `Editor::select_all` (no single `edtui` action for it).
    SelectAll,
    /// `Ctrl+F` -- opens the built-in search box (`Editor::start_search`),
    /// or gives it focus back if it's open but the text has focus. Never
    /// resolved while the box itself has focus -- `handle_editor_key`
    /// routes every key to `handle_search_key` then instead.
    Find,
    /// `F3` / `Shift+F3` -- next/previous match of the open search box
    /// from the caret, while the text has focus (VS Code's own keys for
    /// this). A no-op with the box closed.
    FindNext,
    FindPrevious,
    /// `F9` -- the editor's own menu (`Overlay::EditorMenu`), Far-style.
    OpenMenu,
    /// Not one of the bindings above — forward the raw key event to
    /// `Editor::input`.
    Forward,
    /// A key `edtui` can't handle (`edtui_supports_key`) -- swallowed.
    Ignore,
}


/// Resolves a raw key press to an `EditorCommand`. Letters match both
/// cases: some terminals report the Caps-Lock case with `Ctrl` held.
pub fn resolve(key: KeyEvent) -> EditorCommand {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Esc => EditorCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => EditorCommand::Save,
        // Far's editor saves on F2 too.
        KeyCode::F(2) if shift => EditorCommand::SaveAs,
        KeyCode::F(2) => EditorCommand::Save,
        KeyCode::Char('f' | 'F') if ctrl => EditorCommand::Find,
        // Far's editor search keys: `F7` opens the search (as `Ctrl+F`),
        // `Shift+F7`/`Alt+F7` step to the next/previous match (as `F3`/
        // `Shift+F3`).
        KeyCode::F(7) if shift => EditorCommand::FindNext,
        KeyCode::F(7) if key.modifiers.contains(KeyModifiers::ALT) => EditorCommand::FindPrevious,
        KeyCode::F(7) => EditorCommand::Find,
        KeyCode::Char('a' | 'A') if ctrl => EditorCommand::SelectAll,
        KeyCode::Down if ctrl && !shift => EditorCommand::BlockMove { forward: true },
        KeyCode::Up if ctrl && !shift => EditorCommand::BlockMove { forward: false },
        KeyCode::Left if ctrl && shift => EditorCommand::WordSelect { forward: false },
        KeyCode::Right if ctrl && shift => EditorCommand::WordSelect { forward: true },
        KeyCode::F(9) => EditorCommand::OpenMenu,
        KeyCode::F(3) if shift => EditorCommand::FindPrevious,
        KeyCode::F(3) => EditorCommand::FindNext,
        _ if edtui_supports_key(key.code) => EditorCommand::Forward,
        _ => EditorCommand::Ignore,
    }
}

/// A key for the text itself, the same in every editor -- F4's and each
/// pane of Compare and the conflict resolver: `Ctrl+Shift+Left`/`Right`
/// (word-wise selection), `Ctrl+Up`/`Down` (by blocks of code), `Ctrl+A`,
/// and everything `edtui` handles.
/// Compare used to forward keys straight to `Editor::input`, so word
/// selection and `Ctrl+A` did nothing there. Keys that need the app (save,
/// search, menu, close) are the caller's; they're ignored here.
pub fn text_key(editor: &mut Editor, key: KeyEvent) {
    match resolve(key) {
        EditorCommand::SelectAll => editor.select_all(),
        // Standard-only logic; under Vim the raw key goes to `edtui`,
        // where it's unbound. History: docs/history/editor-keymap.md.
        EditorCommand::WordSelect { forward } if editor.keymap_mode() == EditorKeymapMode::Standard => editor.extend_word_selection(forward),
        EditorCommand::BlockMove { forward } if editor.keymap_mode() == EditorKeymapMode::Standard => editor.move_by_block(forward),
        EditorCommand::WordSelect { .. } | EditorCommand::BlockMove { .. } | EditorCommand::Forward => editor.input(key),
        _ => {}
    }
}


/// `Esc` in an editor pane: cancels a selection if there is one (`true`),
/// as in F4, rather than closing Compare or the resolver with it.
pub fn cancel_selection(editor: &mut Editor, key: KeyEvent) -> bool {
    if !editor.has_selection() {
        return false;
    }
    editor.input(key);
    true
}


/// The keys `edtui`'s own key conversion handles; it hits
/// `unimplemented!()` for any other (`F10` crashed the app). An
/// allow-list, so an `edtui` upgrade only needs it extended. History: docs/history/editor-keymap.md.
pub(crate) fn edtui_supports_key(code: KeyCode) -> bool {
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


/// Key handling while a file is open in the editor. A focused search box
/// gets every key (`handle_search_key`). `Esc` closes an open search box
/// first, then cancels a selection, then closes the editor (asking first
/// if there are unsaved changes). Everything `edtui` understands is
/// forwarded to `Editor::input`; anything else is ignored.
pub fn handle_editor_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if matches!(&app.mode, Mode::Editing(editor) if editor.is_searching()) {
        return handle_search_key(app, key);
    }

    if app.editor_save_as.is_some() {
        save_as_key(app, key);
        return Ok(Effect::None);
    }

    let command = resolve(key);
    debug!(?key, ?command, "editor key");

    // `Esc` with the search box open but the text focused closes just
    // the box, VS Code-style, before it can mean "cancel the selection"
    // or "close the editor" -- the caret stays where it is.
    if command == EditorCommand::Close {
        if let Mode::Editing(editor) = &mut app.mode {
            if editor.search_box_open() {
                editor.close_search_box();
                return Ok(Effect::None);
            }
        }
    }

    if command == EditorCommand::Close {
        let has_selection = matches!(&app.mode, Mode::Editing(editor) if editor.has_selection());
        if has_selection {
            let Mode::Editing(active_editor) = &mut app.mode else {
                return Ok(Effect::None);
            };
            active_editor.input(key);
            return Ok(Effect::None);
        }
        close_editor_or_confirm(app)?;
        return Ok(Effect::None);
    }

    if command == EditorCommand::OpenMenu {
        app.overlay = Some(Overlay::EditorMenu(super::menu::open_editor_menu()));
        return Ok(Effect::None);
    }

    let Mode::Editing(active_editor) = &mut app.mode else {
        return Ok(Effect::None);
    };

    match command {
        EditorCommand::Close => unreachable!("handled above"),
        EditorCommand::OpenMenu => unreachable!("handled above"),
        // A failed save is shown, not returned: an `Err` from a key handler
        // ends the event loop.
        EditorCommand::Save => match active_editor.save() {
            Ok(()) => {
                // Saving refreshes a linked Markdown preview.
                if let Some(preview) = &mut app.markdown_edit_preview {
                    preview.reload();
                }
            }
            Err(err) => app.notice = Some(Notice::error(format!("Save failed: {err}"))),
        },
        EditorCommand::SaveAs => app.editor_save_as = Some(PathEdit::save_as(active_editor.path())),
        EditorCommand::Find => active_editor.start_search(),
        EditorCommand::FindNext => active_editor.search_next(),
        EditorCommand::FindPrevious => active_editor.search_previous(),
        EditorCommand::SelectAll | EditorCommand::WordSelect { .. } | EditorCommand::BlockMove { .. } | EditorCommand::Forward => text_key(active_editor, key),
        EditorCommand::Ignore => {}
    }

    Ok(Effect::None)
}


/// A key in the `Shift+F2` field: `Enter` saves there
/// (`PathEdit::save_editor_as`), `Esc` puts the title back. A failure
/// keeps the field open, with a notice.
fn save_as_key(app: &mut App, key: KeyEvent) {
    let (Some(edit), Mode::Editing(editor)) = (app.editor_save_as.as_mut(), &mut app.mode) else {
        app.editor_save_as = None;
        return;
    };
    match edit.key(key) {
        PathEditKey::Editing => {}
        PathEditKey::Cancel => app.editor_save_as = None,
        PathEditKey::Submit(path) => match edit.save_editor_as(editor, path) {
            Ok(()) => {
                app.editor_save_as = None;
                if let Some(preview) = &mut app.markdown_edit_preview {
                    preview.reload();
                }
            }
            Err(err) => app.notice = Some(Notice::error(err.to_string())),
        },
    }
}


/// Key handling while the `Ctrl+F` box has focus. The box is a full
/// single-line text field (`Editor::search_edit_key`) that filters
/// matches live. `Enter`/`Shift+Enter` go to the next/previous match (VS
/// Code); `Up`/`Down` browse the search history, like a shell; `End` at
/// the end of the query accepts the ghost-text suggestion; `Esc` closes
/// the box and records a non-empty query. History: docs/history/editor-keymap.md.
fn handle_search_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Mode::Editing(active_editor) = &mut app.mode else {
        return Ok(Effect::None);
    };

    match key.code {
        KeyCode::Esc => {
            let query = active_editor.search_query();
            active_editor.stop_search();
            if !query.is_empty() {
                find_history::record_history(&mut app.search_history, &query);
            }
            // Saved to disk once at exit (`main.rs`), not here -- this
            // handler is unit-tested and would write into the cwd.
        }
        KeyCode::Up => active_editor.search_history_up(&app.search_history),
        KeyCode::Down => active_editor.search_history_down(&app.search_history),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => active_editor.search_previous(),
        KeyCode::Enter => active_editor.search_next(),
        KeyCode::End if active_editor.search_cursor_at_end() => {
            let query = active_editor.search_query();
            if let Some(suggestion) = find_history::suggest(&app.search_history, &query) {
                let suggestion = suggestion.to_string();
                active_editor.accept_search_suggestion(&suggestion);
            }
        }
        _ => {
            active_editor.search_edit_key(key);
        }
    }

    Ok(Effect::None)
}


/// Closes the editor if the buffer is clean, else opens
/// `Overlay::ConfirmDiscard`. Also used by the embedded Markdown preview's
/// own `Esc`/`F3`, which close the whole session.
pub(crate) fn close_editor_or_confirm(app: &mut App) -> Result<()> {
    let Mode::Editing(editor) = &app.mode else {
        return Ok(());
    };

    if !editor.is_dirty() {
        return return_from_editor(app);
    }

    debug!("editor close: unsaved changes, asking to confirm discard");
    app.overlay = Some(Overlay::ConfirmDiscard);
    Ok(())
}


/// The editor is closing for good: reloads the active panel and returns
/// to wherever `F4` came from -- `Overlay::FindFile` (`editor_return_to`),
/// `Overlay::UserMenu` after writing the scratch file back
/// (`user_menu_command_edit`), else browsing. Clears a linked Markdown
/// preview; mouse capture follows the mode. Cancelling the discard
/// prompt doesn't come here -- the editor hasn't closed.
fn return_from_editor(app: &mut App) -> Result<()> {
    app.active_panel().reload()?;
    app.markdown_edit_preview = None;
    app.mode = Mode::Browsing;
    app.overlay = if let Some(state) = app.editor_return_to.take() {
        Some(Overlay::FindFile(state))
    } else {
        app.user_menu_command_edit.take().map(|edit| Overlay::UserMenu(explorer::finish_command_edit(edit)))
    };
    Ok(())
}


/// Key handling on the "discard unsaved changes?" prompt: `Y` discards
/// and returns to browsing, `N`/`Esc` cancels back into the editor with
/// nothing lost, anything else is ignored.
pub fn handle_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let answer = yes_no::answer(key);
    debug!(?key, ?answer, "confirm-discard key");

    match answer {
        Answer::Yes => return_from_editor(app)?,
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }

    Ok(Effect::None)
}


#[cfg(test)]
mod tests;
