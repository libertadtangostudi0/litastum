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
use tracing::debug;

use crate::app::{App, Mode, TransferOp};
use crate::text_field;
use super::{fs_ops, keymap};


/// Key handling on the F8 "delete this?" prompt: `Y` actually deletes
/// (a file via `fs::remove_file`, a directory recursively via
/// `fs::remove_dir_all` — no separate "is it empty" case, matching Far
/// Manager's own F8 which recurses without asking twice) and reloads
/// the panel; `N`/`Esc` cancels with nothing touched. A failed delete
/// (permissions, a file in use, ...) is logged rather than crashing —
/// there's no status-bar message surface yet to show it to the user
/// (see `TODO.md`'s non-UTF-8-file gap, same underlying limitation).
pub fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use keymap::ConfirmDeleteCommand;

    let Mode::ConfirmDelete(pending) = &app.mode else {
        return Ok(());
    };

    let command = keymap::resolve_confirm_delete(key);
    debug!(?key, ?command, path = %pending.path.display(), "confirm-delete key");

    match command {
        ConfirmDeleteCommand::Confirm => {
            let Mode::ConfirmDelete(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::ConfirmDelete above");
            };
            let result = if pending.is_dir {
                fs::remove_dir_all(&pending.path)
            } else {
                fs::remove_file(&pending.path)
            };
            if let Err(err) = result {
                debug!(path = %pending.path.display(), %err, "delete failed");
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
/// cursor. `Enter` performs the transfer (`fs_ops::copy_entry`/
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


/// `Enter` on the transfer prompt: runs the copy/move
/// (`fs_ops::copy_entry`/`move_entry`) and reloads both panels. Split
/// out of `handle_confirm_transfer_key` since it needs to consume
/// `app.mode` via `mem::replace` (to take ownership of `PendingTransfer`
/// without cloning it) rather than just borrow it like every other key
/// on that prompt does.
fn run_confirmed_transfer(app: &mut App) -> Result<()> {
    let Mode::ConfirmTransfer(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        return Ok(());
    };
    let destination = PathBuf::from(pending.destination.trim());
    debug!(
        source = %pending.source.display(),
        destination = %destination.display(),
        op = ?pending.operation,
        "confirm-transfer: running"
    );
    let result = match pending.operation {
        TransferOp::Copy => fs_ops::copy_entry(&pending.source, &destination, pending.is_dir),
        TransferOp::Move => fs_ops::move_entry(&pending.source, &destination, pending.is_dir),
    };
    if let Err(err) = result {
        debug!(source = %pending.source.display(), destination = %destination.display(), %err, "transfer failed");
    }
    for panel in &mut app.panels {
        panel.reload()?;
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, PendingDelete, PendingTransfer};
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("confirm")
    }

    fn scratch_app() -> App {
        test_app(scratch_dir())
    }

    #[test]
    fn confirm_delete_removes_a_file_and_returns_to_browsing() {
        let mut app = scratch_app();
        let file = app.panels[0].path.join("victim.txt");
        fs::write(&file, b"bye").unwrap();
        app.mode = Mode::ConfirmDelete(PendingDelete { path: file.clone(), name: "victim.txt".into(), is_dir: false });

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
        app.mode = Mode::ConfirmDelete(PendingDelete { path: dir.clone(), name: "victim_dir".into(), is_dir: true });

        handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(!dir.exists());
    }

    #[test]
    fn confirm_delete_cancel_leaves_the_file_untouched() {
        let mut app = scratch_app();
        let file = app.panels[0].path.join("keep.txt");
        fs::write(&file, b"stay").unwrap();
        app.mode = Mode::ConfirmDelete(PendingDelete { path: file.clone(), name: "keep.txt".into(), is_dir: false });

        handle_confirm_delete_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(file.exists());
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn confirm_delete_ignores_unrelated_keys_and_stays_open() {
        let mut app = scratch_app();
        let file = app.panels[0].path.join("keep2.txt");
        fs::write(&file, b"stay").unwrap();
        app.mode = Mode::ConfirmDelete(PendingDelete { path: file.clone(), name: "keep2.txt".into(), is_dir: false });

        handle_confirm_delete_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(file.exists());
        assert!(matches!(app.mode, Mode::ConfirmDelete(_)));
    }

    fn pending_transfer(operation: TransferOp, source: PathBuf, destination: String) -> PendingTransfer {
        let cursor = destination.chars().count();
        PendingTransfer {
            operation,
            source,
            name: "irrelevant".into(),
            is_dir: false,
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
}
