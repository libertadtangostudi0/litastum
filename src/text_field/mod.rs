//! Single-line text fields: `TextField` (text + cursor + selection) and
//! the standard key layout it applies (`apply_edit_key`). Used by the
//! command line, the F5/F6 destination, Find file's fields, the editor's
//! `Ctrl+F` box and the F2 menu's forms. The command line takes only
//! part of the layout -- bare arrows there are panel navigation
//! (`.claude/rules/litastum-command-line.md`).
//!
//! Positions are character indices (not byte offsets), so multi-byte
//! UTF-8 text never splits mid-character; `byte_index` is the one place
//! that converts between the two.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::warn;

mod field;
mod undo;

pub use field::TextField;

/// Byte offset of the `char_idx`-th character in `text` (or `text`'s
/// full length, for `char_idx == text.chars().count()` — the
/// end-of-string cursor position, which has no character of its own).
fn byte_index(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len())
}


/// Inserts `c` at `*cursor` and advances the cursor past it.
fn insert_char(text: &mut String, cursor: &mut usize, c: char) {
    let byte = byte_index(text, *cursor);
    text.insert(byte, c);
    *cursor += 1;
}


/// Deletes the character just before `*cursor` (a no-op at the start).
fn backspace(text: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let start = byte_index(text, *cursor - 1);
    let end = byte_index(text, *cursor);
    text.replace_range(start..end, "");
    *cursor -= 1;
}


/// Deletes the character at `*cursor` (a no-op at the end); the cursor
/// itself doesn't move, matching a normal editor's `Delete` key.
fn delete_forward(text: &mut String, cursor: &mut usize) {
    let len = text.chars().count();
    if *cursor >= len {
        return;
    }
    let start = byte_index(text, *cursor);
    let end = byte_index(text, *cursor + 1);
    text.replace_range(start..end, "");
}


/// Moves the cursor one character left, clamped at the start.
fn move_left(cursor: &mut usize) {
    *cursor = cursor.saturating_sub(1);
}


/// Moves the cursor one character right, clamped at the end.
fn move_right(text: &str, cursor: &mut usize) {
    let len = text.chars().count();
    if *cursor < len {
        *cursor += 1;
    }
}


fn move_home(cursor: &mut usize) {
    *cursor = 0;
}


fn move_end(text: &str, cursor: &mut usize) {
    *cursor = text.chars().count();
}


fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}


/// `/` and `\` are word stops of their own, not chained into the next
/// word: `Ctrl+Right` before a `/` jumped the whole next path segment.
/// Only these two -- other punctuation keeps "skip the run and the next
/// word". `.claude/rules/litastum-command-line.md`.
fn is_path_sep(c: char) -> bool {
    c == '/' || c == '\\'
}


/// Moves the cursor to the start of the previous word (`Ctrl+Left`) —
/// skips any run of non-word characters (spaces, punctuation, ...)
/// immediately to the left first, then the word itself, same as a
/// standard text editor's word-left. A `/` or `\` right before the
/// cursor is its own stop instead (`is_path_sep`'s own doc comment) —
/// checked first, before the general run-skip, so it doesn't get
/// swallowed into either side of it.
fn move_word_left(text: &str, cursor: &mut usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = *cursor;

    if i > 0 && is_path_sep(chars[i - 1]) {
        *cursor = i - 1;
        return;
    }

    while i > 0 && !is_word_char(chars[i - 1]) && !is_path_sep(chars[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word_char(chars[i - 1]) {
        i -= 1;
    }
    *cursor = i;
}


/// Moves the cursor to the start of the next word (`Ctrl+Right`) —
/// mirror of `move_word_left`.
fn move_word_right(text: &str, cursor: &mut usize) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = *cursor;

    if i < len && is_path_sep(chars[i]) {
        *cursor = i + 1;
        return;
    }

    while i < len && !is_word_char(chars[i]) && !is_path_sep(chars[i]) {
        i += 1;
    }
    while i < len && is_word_char(chars[i]) {
        i += 1;
    }
    *cursor = i;
}


// -- Selection (Shift+Left/Right) --------------------------------------
//
// A selection is `anchor..cursor` (order-independent -- `selection_range`
// normalizes it): `anchor` is where Shift+arrow started, `cursor` is the
// live end that keeps moving. `None` means no selection.

/// The selected char-range, `start <= end`, regardless of which
/// direction the selection was extended in.
fn selection_range(anchor: usize, cursor: usize) -> (usize, usize) {
    if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) }
}


/// Removes chars `start..end` from `text` (char indices, `start <= end`).
fn delete_range(text: &mut String, start: usize, end: usize) {
    let start_b = byte_index(text, start);
    let end_b = byte_index(text, end);
    text.replace_range(start_b..end_b, "");
}


/// `Shift+Left`: starts a selection at the current cursor if none is
/// active yet, then moves the cursor (the selection's live end) left.
fn extend_selection_left(cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_left(cursor);
}


/// `Shift+Right` — mirror of `extend_selection_left`.
fn extend_selection_right(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_right(text, cursor);
}


/// `Ctrl+Shift+Left` — word-wise version of `extend_selection_left`,
/// same anchor-starting behavior, extending by a whole word
/// (`move_word_left`) instead of one character.
fn extend_selection_word_left(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_word_left(text, cursor);
}


/// `Ctrl+Shift+Right` — mirror of `extend_selection_word_left`.
fn extend_selection_word_right(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_word_right(text, cursor);
}


/// Plain `Left` with a selection active: collapses to the selection's
/// start instead of moving one more character, matching a standard
/// text editor. With no selection, just moves left as usual.
fn collapse_selection_left(cursor: &mut usize, anchor: &mut Option<usize>) {
    match anchor.take() {
        Some(a) => *cursor = (*cursor).min(a),
        None => move_left(cursor),
    }
}


/// Plain `Right` with a selection active — mirror of
/// `collapse_selection_left`, collapsing to the selection's end.
fn collapse_selection_right(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    match anchor.take() {
        Some(a) => *cursor = (*cursor).max(a),
        None => move_right(text, cursor),
    }
}


/// If a selection is active, deletes it, moves the cursor to where it
/// started, and clears `anchor` — returns `true`. Otherwise leaves
/// everything untouched and returns `false`, so callers can fall back
/// to their own single-character `backspace`/`delete_forward`.
fn delete_selection(text: &mut String, cursor: &mut usize, anchor: &mut Option<usize>) -> bool {
    let Some(a) = anchor.take() else {
        return false;
    };
    let (start, end) = selection_range(a, *cursor);
    delete_range(text, start, end);
    *cursor = start;
    true
}


/// The active selection's own text, if there is one and it's non-empty
/// -- split out of `copy_selection` as a pure function so the actual
/// substring arithmetic has real unit coverage without touching the real
/// OS clipboard (this codebase deliberately avoids that elsewhere too --
/// `editor::clipboard`'s own tests never call a real `Clipboard::new()`
/// either, since it isn't guaranteed to be available wherever the test
/// suite happens to run).
fn selected_text(text: &str, cursor: usize, anchor: Option<usize>) -> Option<String> {
    let anchor = anchor?;
    let (start, end) = selection_range(anchor, cursor);
    let selected: String = text.chars().skip(start).take(end - start).collect();
    (!selected.is_empty()).then_some(selected)
}


/// Copies the active selection (nothing, if there isn't one) to the real
/// OS clipboard -- a failure (no clipboard available in this
/// environment, or the OS call itself failing) is only logged, same
/// "don't fail the keystroke over it" rule
/// `editor::clipboard::OsClipboardBridge` already follows.
fn copy_selection(text: &str, cursor: usize, anchor: Option<usize>) {
    let Some(selected) = selected_text(text, cursor, anchor) else {
        return;
    };
    let Some(mut clipboard) = os_clipboard() else {
        return;
    };
    if let Err(err) = clipboard.set_text(selected) {
        warn!(%err, "text field: clipboard set_text failed");
    }
}


/// Pastes the real OS clipboard's text at the cursor, replacing the
/// active selection first if there is one -- the same "replace on type"
/// convention typing follows. Control characters (line breaks, tabs)
/// are dropped: every user of this is a single-line field. A missing/
/// unavailable clipboard or non-text contents is only logged, same as
/// `copy_selection`.
fn paste_clipboard(text: &mut String, cursor: &mut usize, anchor: &mut Option<usize>) {
    let Some(mut clipboard) = os_clipboard() else {
        return;
    };
    let pasted = match clipboard.get_text() {
        Ok(pasted) => pasted,
        Err(err) => {
            warn!(%err, "text field: clipboard get_text failed");
            return;
        }
    };
    delete_selection(text, cursor, anchor);
    for c in pasted.chars().filter(|c| !c.is_control()) {
        insert_char(text, cursor, c);
    }
}


/// The real OS clipboard -- `None` if it isn't available (logged), and
/// always `None` in a test build: a test pressing `Ctrl+C`/`Ctrl+X` must
/// never overwrite the clipboard of whoever happens to be running the
/// suite (the same isolation rule `theming::config::limits()` follows
/// for `config.json`).
fn os_clipboard() -> Option<arboard::Clipboard> {
    if cfg!(test) {
        return None;
    }
    match arboard::Clipboard::new() {
        Ok(clipboard) => Some(clipboard),
        Err(err) => {
            warn!(%err, "text field: clipboard unavailable");
            None
        }
    }
}


/// What `apply_edit_key` did with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOutcome {
    /// The text itself changed (typing, deleting, replacing a selection).
    TextChanged,
    /// Handled, but the text itself didn't change (a cursor/selection
    /// move, a copy).
    NoTextChange,
    /// Not an editing key -- left for the caller's own bindings.
    Unhandled,
}

/// The standard single-line editing keys: `Backspace`/`Delete` (a
/// selection first), `Shift`/`Ctrl+Shift` + arrows select, `Ctrl` + arrows
/// jump words, plain arrows (collapsing a selection), `Home`/`End`,
/// `Ctrl+C`/`X`/`V` with the OS clipboard, typing over a selection. Shared
/// by Find file and the editor's `Ctrl+F` box ("the same as in Find
/// file"). `Ctrl+Shift` is checked before `Shift` (both have `SHIFT`),
/// which once made word selection character-wise.
fn apply_edit_key(text: &mut String, cursor: &mut usize, anchor: &mut Option<usize>, key: KeyEvent) -> EditOutcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Backspace => {
            if !delete_selection(text, cursor, anchor) {
                backspace(text, cursor);
            }
            EditOutcome::TextChanged
        }
        KeyCode::Delete => {
            if !delete_selection(text, cursor, anchor) {
                delete_forward(text, cursor);
            }
            EditOutcome::TextChanged
        }
        // Clipboard -- reported directly as missing from the editor's
        // `Ctrl+F` box ("Ctrl+X doesn't work on the selected text"); the
        // F5/F6 destination field already had these, so they moved here
        // from there and every field using this function gets them.
        KeyCode::Char('c' | 'C') if ctrl => {
            copy_selection(text, *cursor, *anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Char('x' | 'X') if ctrl => {
            copy_selection(text, *cursor, *anchor);
            if delete_selection(text, cursor, anchor) {
                EditOutcome::TextChanged
            } else {
                EditOutcome::NoTextChange
            }
        }
        KeyCode::Char('v' | 'V') if ctrl => {
            paste_clipboard(text, cursor, anchor);
            EditOutcome::TextChanged
        }
        KeyCode::Char(c) if !ctrl => {
            delete_selection(text, cursor, anchor);
            insert_char(text, cursor, c);
            EditOutcome::TextChanged
        }
        KeyCode::Left if ctrl && shift => {
            extend_selection_word_left(text, cursor, anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Right if ctrl && shift => {
            extend_selection_word_right(text, cursor, anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Left if shift => {
            extend_selection_left(cursor, anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Right if shift => {
            extend_selection_right(text, cursor, anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Left if ctrl => {
            *anchor = None;
            move_word_left(text, cursor);
            EditOutcome::NoTextChange
        }
        KeyCode::Right if ctrl => {
            *anchor = None;
            move_word_right(text, cursor);
            EditOutcome::NoTextChange
        }
        KeyCode::Left => {
            collapse_selection_left(cursor, anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Right => {
            collapse_selection_right(text, cursor, anchor);
            EditOutcome::NoTextChange
        }
        KeyCode::Home => {
            *anchor = None;
            move_home(cursor);
            EditOutcome::NoTextChange
        }
        KeyCode::End => {
            *anchor = None;
            move_end(text, cursor);
            EditOutcome::NoTextChange
        }
        _ => EditOutcome::Unhandled,
    }
}


#[cfg(test)]
mod tests;
