//! Key handling *and rendering* for the two ephemeral filesystem-action
//! popups: F8's "delete this?" (`Mode::ConfirmDelete`) and
//! F5/F6/Shift+F6's "copy/move/rename to?" (`Mode::ConfirmTransfer`).
//! Grouped in one module since both are small, short-lived
//! confirmations over a `Pending*` struct from `app.rs`, and neither
//! owns a whole subsystem the way `theme_menu.rs`/`menu.rs`/
//! `command_line.rs` do — moved out of `main.rs`/`ui.rs` for the same
//! reason those were: each mode's key handling and rendering lives
//! with the concern it belongs to, rather than in a general dispatcher.

use std::fs;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, warn};

use crate::app::{App, Mode, TransferOp};
use crate::text_field;
use super::{fs_ops, keymap};


/// Key handling on the F8 "delete this?" prompt: `Y` actually deletes
/// every entry in `pending.entries` (a file via `fs::remove_file`, a
/// directory recursively via `fs::remove_dir_all` — no separate "is it
/// empty" case, matching Far Manager's own F8 which recurses without
/// asking twice) and reloads the panel; `N`/`Esc` cancels with nothing
/// touched. A failed delete (permissions, a file in use, ...) is only
/// logged, same as `confirm::run_confirmed_transfer` — doesn't stop the
/// rest of `entries` from being attempted, and there's no status-bar
/// message surface yet to show it to the user (see `TODO/editor.md`'s
/// non-UTF-8-file gap, same underlying limitation).
pub fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use keymap::ConfirmDeleteCommand;

    let Mode::ConfirmDelete(pending) = &app.mode else {
        return Ok(());
    };

    let command = keymap::resolve_confirm_delete(key);
    debug!(?key, ?command, count = pending.entries.len(), "confirm-delete key");

    match command {
        ConfirmDeleteCommand::Confirm => {
            let Mode::ConfirmDelete(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::ConfirmDelete above");
            };
            for entry in &pending.entries {
                let result = if entry.is_dir {
                    fs::remove_dir_all(&entry.path)
                } else {
                    fs::remove_file(&entry.path)
                };
                if let Err(err) = result {
                    debug!(path = %entry.path.display(), %err, "delete failed");
                }
            }
            app.active_panel().reload()?;
        }
        ConfirmDeleteCommand::Cancel => app.mode = Mode::Browsing,
        ConfirmDeleteCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling on the F5/F6 "copy/move to?" prompt: the destination
/// line gets a real cursor (`text_field.rs`, not the command line's
/// own append/backspace-only editing — see that module's doc for why
/// this popup gets one and the always-live command line doesn't) —
/// `Left`/`Right` move a character, `Ctrl+Left`/`Ctrl+Right` a word,
/// `Home`/`End` to the edges, `Backspace`/`Delete` remove around the
/// cursor, `Ctrl+C`/`Ctrl+X`/`Ctrl+V` copy/cut/paste the active
/// selection against the real OS clipboard. `Enter` performs the transfer (`fs_ops::copy_entry`/
/// `move_entry`) and reloads *both* panels (the destination side
/// always needs it, and a move also changes the source side); `Esc`
/// cancels with nothing touched. A failed transfer is only logged, same
/// as `handle_confirm_delete_key` — no status-bar surface exists yet.
pub fn handle_confirm_transfer_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Enter {
        return run_confirmed_transfer(app);
    }
    if key.code == KeyCode::Esc {
        app.mode = Mode::Browsing;
        return Ok(());
    }

    // Everything past this point only edits the destination field, so
    // borrow `pending` once instead of re-matching `Mode::ConfirmTransfer`
    // per key (each of the arms below used to do its own `if let`).
    let Mode::ConfirmTransfer(pending) = &mut app.mode else {
        return Ok(());
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        // Backspace/Delete remove the active selection instead of one
        // character, if there is one -- text_field::delete_selection
        // reports whether it did anything, so the single-character path
        // only runs when there wasn't a selection to consume instead.
        KeyCode::Backspace => {
            let removed_selection =
                text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            if !removed_selection {
                text_field::backspace(&mut pending.destination, &mut pending.cursor);
            }
        }
        KeyCode::Delete => {
            let removed_selection =
                text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            if !removed_selection {
                text_field::delete_forward(&mut pending.destination, &mut pending.cursor);
            }
        }
        // Shift+Left/Right (selection) is checked ahead of Ctrl+Left/
        // Right and plain Left/Right below, same reason Ctrl+P is
        // checked ahead of the browsing keymap table -- KeyCode::Left
        // alone can't distinguish "extend selection" from "move" or
        // "jump a word".
        KeyCode::Left if shift => {
            text_field::extend_selection_left(&mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Right if shift => {
            text_field::extend_selection_right(&pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Left if ctrl => {
            pending.selection_anchor = None;
            text_field::move_word_left(&pending.destination, &mut pending.cursor);
        }
        KeyCode::Right if ctrl => {
            pending.selection_anchor = None;
            text_field::move_word_right(&pending.destination, &mut pending.cursor);
        }
        // Plain Left/Right with a selection active collapses to that
        // selection's near edge (standard editor behavior) rather than
        // moving one further character past it.
        KeyCode::Left => {
            text_field::collapse_selection_left(&mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Right => {
            text_field::collapse_selection_right(&pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Home => {
            pending.selection_anchor = None;
            text_field::move_home(&mut pending.cursor);
        }
        KeyCode::End => {
            pending.selection_anchor = None;
            text_field::move_end(&pending.destination, &mut pending.cursor);
        }
        // Ctrl+C/X/V against the real OS clipboard -- reported directly
        // as a real gap: this field has a selection (Shift+Left/Right
        // above) but no way to actually get part of a long path out of
        // it. Same `arboard` dependency `editor::clipboard` already
        // bridges into `edtui` with, used directly here instead since
        // this popup isn't an `edtui` buffer at all.
        KeyCode::Char('c') if ctrl => copy_selection_to_clipboard(&pending.destination, pending.cursor, pending.selection_anchor),
        KeyCode::Char('x') if ctrl => {
            copy_selection_to_clipboard(&pending.destination, pending.cursor, pending.selection_anchor);
            text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Char('v') if ctrl => paste_from_clipboard(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor),
        // Typing over an active selection replaces it, like any normal
        // text field -- delete it first, then insert at the (now
        // collapsed) cursor.
        KeyCode::Char(c) if !ctrl => {
            text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            text_field::insert_char(&mut pending.destination, &mut pending.cursor, c);
        }
        _ => {}
    }

    Ok(())
}


/// The active selection's own text, if there is one and it's non-empty
/// -- split out of `copy_selection_to_clipboard` as a pure function so
/// the actual substring arithmetic has real unit coverage without
/// touching the real OS clipboard (this codebase deliberately avoids
/// that elsewhere too -- `editor::clipboard`'s own tests never call a
/// real `Clipboard::new()` either, since it isn't guaranteed to be
/// available wherever the test suite happens to run).
fn selected_text(text: &str, cursor: usize, selection_anchor: Option<usize>) -> Option<String> {
    let anchor = selection_anchor?;
    let (start, end) = text_field::selection_range(anchor, cursor);
    let selected: String = text.chars().skip(start).take(end - start).collect();
    (!selected.is_empty()).then_some(selected)
}


/// Copies the destination field's own active selection (nothing, if
/// there isn't one) to the real OS clipboard -- a failure (no clipboard
/// available in this environment, or the OS call itself failing) is
/// only logged, same "don't fail the keystroke over it" rule
/// `editor::clipboard::OsClipboardBridge` already follows.
fn copy_selection_to_clipboard(text: &str, cursor: usize, selection_anchor: Option<usize>) {
    let Some(selected) = selected_text(text, cursor, selection_anchor) else {
        return;
    };
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            if let Err(err) = clipboard.set_text(selected) {
                warn!(%err, "confirm transfer: clipboard set_text failed");
            }
        }
        Err(err) => warn!(%err, "confirm transfer: clipboard unavailable"),
    }
}


/// Pastes the real OS clipboard's text into the destination field,
/// replacing the active selection first if there is one -- same
/// "replace on type" convention the plain `KeyCode::Char` arm below
/// already follows. A missing/unavailable clipboard or non-text
/// contents is only logged, same as `copy_selection_to_clipboard`.
fn paste_from_clipboard(text: &mut String, cursor: &mut usize, selection_anchor: &mut Option<usize>) {
    let pasted = match arboard::Clipboard::new() {
        Ok(mut clipboard) => match clipboard.get_text() {
            Ok(pasted) => pasted,
            Err(err) => {
                warn!(%err, "confirm transfer: clipboard get_text failed");
                return;
            }
        },
        Err(err) => {
            warn!(%err, "confirm transfer: clipboard unavailable");
            return;
        }
    };
    text_field::delete_selection(text, cursor, selection_anchor);
    for c in pasted.chars().filter(|c| !c.is_control()) {
        text_field::insert_char(text, cursor, c);
    }
}


/// `Enter` on the transfer prompt: runs the copy/move
/// (`fs_ops::copy_entry`/`move_entry`) for every entry in
/// `pending.sources` and reloads both panels. Split out of
/// `handle_confirm_transfer_key` since it needs to consume `app.mode`
/// via `mem::replace` (to take ownership of `PendingTransfer` without
/// cloning it) rather than just borrow it like every other key on that
/// prompt does.
///
/// A single source treats `destination` as the *full* target path,
/// exactly as before multi-select existed (this is what lets a
/// single-entry transfer double as a rename, editing the trailing
/// filename). Several sources have no one path that could do that for
/// all of them, so `destination` is instead the target *directory*,
/// with each source's own name joined onto it individually — a failure
/// on one entry (permissions, a name collision, ...) is only logged,
/// same as the single-entry case, and doesn't stop the rest from being
/// attempted.
fn run_confirmed_transfer(app: &mut App) -> Result<()> {
    let Mode::ConfirmTransfer(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        return Ok(());
    };
    let destination_input = PathBuf::from(pending.destination.trim());

    for source in &pending.sources {
        let destination = if pending.sources.len() == 1 {
            destination_input.clone()
        } else {
            destination_input.join(&source.name)
        };
        debug!(
            source = %source.path.display(),
            destination = %destination.display(),
            op = ?pending.operation,
            "confirm-transfer: running"
        );
        let result = match pending.operation {
            TransferOp::Copy => fs_ops::copy_entry(&source.path, &destination, source.is_dir),
            TransferOp::Move => fs_ops::move_entry(&source.path, &destination, source.is_dir),
        };
        if let Err(err) = result {
            debug!(source = %source.path.display(), destination = %destination.display(), %err, "transfer failed");
        }
    }

    for panel in &mut app.panels {
        panel.reload()?;
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, DeleteEntry, PendingDelete, PendingTransfer, TransferSource};
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("confirm")
    }

    fn scratch_app() -> App {
        test_app(scratch_dir())
    }

    fn pending_delete(path: PathBuf, name: &str, is_dir: bool, size: u64) -> PendingDelete {
        PendingDelete { entries: vec![DeleteEntry { path, name: name.into(), is_dir, size }] }
    }

    #[test]
    fn confirm_delete_removes_a_file_and_returns_to_browsing() {
        let mut app = scratch_app();
        let file = app.panels[0].path.join("victim.txt");
        fs::write(&file, b"bye").unwrap();
        app.mode = Mode::ConfirmDelete(pending_delete(file.clone(), "victim.txt", false, 3));

        handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(!file.exists());
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn confirm_delete_removes_a_directory_recursively() {
        let mut app = scratch_app();
        let dir = app.panels[0].path.join("victim_dir");
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested").join("f.txt"), b"x").unwrap();
        app.mode = Mode::ConfirmDelete(pending_delete(dir.clone(), "victim_dir", true, 0));

        handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(!dir.exists());
    }

    #[test]
    fn confirm_delete_cancel_leaves_the_file_untouched() {
        let mut app = scratch_app();
        let file = app.panels[0].path.join("keep.txt");
        fs::write(&file, b"stay").unwrap();
        app.mode = Mode::ConfirmDelete(pending_delete(file.clone(), "keep.txt", false, 4));

        handle_confirm_delete_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(file.exists());
        assert!(matches!(app.mode, Mode::Browsing));
    }

    /// Regression coverage for the real request: F8 should delete every
    /// marked entry, not just the one under the cursor.
    #[test]
    fn confirm_delete_removes_every_entry_in_a_multi_entry_prompt() {
        let mut app = scratch_app();
        let a = app.panels[0].path.join("a.txt");
        let b = app.panels[0].path.join("b.txt");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        app.mode = Mode::ConfirmDelete(PendingDelete {
            entries: vec![
                DeleteEntry { path: a.clone(), name: "a.txt".into(), is_dir: false, size: 1 },
                DeleteEntry { path: b.clone(), name: "b.txt".into(), is_dir: false, size: 1 },
            ],
        });

        handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(!a.exists());
        assert!(!b.exists());
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn confirm_delete_ignores_unrelated_keys_and_stays_open() {
        let mut app = scratch_app();
        let file = app.panels[0].path.join("keep2.txt");
        fs::write(&file, b"stay").unwrap();
        app.mode = Mode::ConfirmDelete(pending_delete(file.clone(), "keep2.txt", false, 4));

        handle_confirm_delete_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(file.exists());
        assert!(matches!(app.mode, Mode::ConfirmDelete(_)));
    }

    mod selected_text_tests {
        use super::*;

        #[test]
        fn no_anchor_means_no_selection() {
            assert_eq!(selected_text("W:\\path\\to\\file.txt", 3, None), None);
        }

        #[test]
        fn a_collapsed_selection_is_treated_as_none() {
            assert_eq!(selected_text("W:\\path\\to\\file.txt", 5, Some(5)), None);
        }

        #[test]
        fn extracts_the_selected_span_regardless_of_anchor_cursor_order() {
            // "W:\path\to\file.txt" -- selecting just "path" (indices 3..7).
            assert_eq!(selected_text("W:\\path\\to\\file.txt", 7, Some(3)), Some("path".to_string()));
            // Same span, cursor and anchor swapped -- selection_range
            // normalizes either order, this should too.
            assert_eq!(selected_text("W:\\path\\to\\file.txt", 3, Some(7)), Some("path".to_string()));
        }
    }

    fn pending_transfer(operation: TransferOp, source: PathBuf, destination: String) -> PendingTransfer {
        let cursor = destination.chars().count();
        PendingTransfer {
            operation,
            sources: vec![TransferSource { path: source, name: "irrelevant".into(), is_dir: false }],
            destination,
            cursor,
            selection_anchor: None,
        }
    }

    #[test]
    fn confirm_transfer_enter_copies_and_keeps_the_source() {
        let mut app = scratch_app();
        let src = app.panels[0].path.join("source.txt");
        fs::write(&src, b"hello").unwrap();
        let dst = app.panels[0].path.join("dest.txt");
        app.mode = Mode::ConfirmTransfer(pending_transfer(TransferOp::Copy, src.clone(), dst.to_string_lossy().into_owned()));

        handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(src.exists(), "copy should leave the source alone");
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn confirm_transfer_enter_moves_and_removes_the_source() {
        let mut app = scratch_app();
        let src = app.panels[0].path.join("source.txt");
        fs::write(&src, b"hello").unwrap();
        let dst = app.panels[0].path.join("dest.txt");
        app.mode = Mode::ConfirmTransfer(pending_transfer(TransferOp::Move, src.clone(), dst.to_string_lossy().into_owned()));

        handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(!src.exists(), "move should remove the source");
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn confirm_transfer_esc_cancels_without_touching_the_filesystem() {
        let mut app = scratch_app();
        let src = app.panels[0].path.join("source.txt");
        fs::write(&src, b"hello").unwrap();
        let dst = app.panels[0].path.join("dest.txt");
        app.mode = Mode::ConfirmTransfer(pending_transfer(TransferOp::Copy, src.clone(), dst.to_string_lossy().into_owned()));

        handle_confirm_transfer_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(src.exists());
        assert!(!dst.exists());
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn confirm_transfer_typing_edits_the_destination_in_place() {
        let mut app = scratch_app();
        app.mode = Mode::ConfirmTransfer(pending_transfer(TransferOp::Copy, PathBuf::from("src"), "dest".to_string()));

        handle_confirm_transfer_key(&mut app, key(KeyCode::Char('!'))).unwrap();

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("should stay in ConfirmTransfer");
        };
        assert_eq!(pending.destination, "dest!");
        assert_eq!(pending.cursor, 5);
    }

    #[test]
    fn confirm_transfer_a_failed_move_still_returns_to_browsing_without_panicking() {
        let mut app = scratch_app();
        let missing_src = app.panels[0].path.join("does-not-exist.txt");
        let dst = app.panels[0].path.join("dest.txt");
        app.mode = Mode::ConfirmTransfer(pending_transfer(TransferOp::Move, missing_src, dst.to_string_lossy().into_owned()));

        handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert!(!dst.exists());
    }

    /// Regression coverage for the real request: F5 should copy every
    /// marked entry, not just the one under the cursor -- several
    /// `sources` join `destination` (a target *directory* here, not a
    /// full file path) with each source's own name individually.
    #[test]
    fn confirm_transfer_enter_copies_every_marked_source_into_the_destination_directory() {
        let mut app = scratch_app();
        let src_dir = app.panels[0].path.clone();
        fs::write(src_dir.join("a.txt"), b"aaa").unwrap();
        fs::write(src_dir.join("b.txt"), b"bbb").unwrap();
        let dst_dir = app.panels[0].path.join("dest");
        fs::create_dir_all(&dst_dir).unwrap();
        app.mode = Mode::ConfirmTransfer(PendingTransfer {
            operation: TransferOp::Copy,
            sources: vec![
                TransferSource { path: src_dir.join("a.txt"), name: "a.txt".into(), is_dir: false },
                TransferSource { path: src_dir.join("b.txt"), name: "b.txt".into(), is_dir: false },
            ],
            destination: dst_dir.to_string_lossy().into_owned(),
            cursor: 0,
            selection_anchor: None,
        });

        handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(fs::read(dst_dir.join("a.txt")).unwrap(), b"aaa");
        assert_eq!(fs::read(dst_dir.join("b.txt")).unwrap(), b"bbb");
        assert!(src_dir.join("a.txt").exists(), "copy should leave the sources alone");
        assert!(src_dir.join("b.txt").exists());
    }
}
