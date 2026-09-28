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
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

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
    text_field::apply_edit_key(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor, key);

    Ok(())
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
mod tests;
