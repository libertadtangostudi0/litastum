//! Keys and rendering for the two short-lived file-action prompts: F8's
//! delete (`Overlay::ConfirmDelete`) and F5/F6/`Shift+F6`'s copy/move/
//! rename (`Overlay::ConfirmTransfer`), each over a `Pending*` from
//! `app.rs`.

use std::fs;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Overlay, TransferOp};
use crate::yes_no::{self, Answer};
use super::fs_ops;


/// The F8 prompt: `Y` deletes every entry (directories recursively, as
/// Far does without asking twice) and reloads; `N`/`Esc` cancel. A failed
/// delete is logged and the rest continue -- there's no message surface
/// to show it yet.
pub fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(Overlay::ConfirmDelete(pending)) = &app.overlay else {
        return Ok(());
    };

    let answer = yes_no::answer(key);
    debug!(?key, ?answer, count = pending.entries.len(), "confirm-delete key");

    match answer {
        Answer::Yes => {
            let Some(Overlay::ConfirmDelete(pending)) = app.overlay.take() else {
                unreachable!("just matched Overlay::ConfirmDelete above");
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
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }

    Ok(())
}


/// Key handling on the F5/F6 "copy/move to?" prompt: the destination
/// is a standard text field (`TextField::apply_key` -- cursor, word
/// moves, selection, OS clipboard). `Enter` performs the transfer (`fs_ops::copy_entry`/
/// `move_entry`) and reloads *both* panels (the destination side
/// always needs it, and a move also changes the source side); `Esc`
/// cancels with nothing touched. A failed transfer is only logged, same
/// as `handle_confirm_delete_key` — no status-bar surface exists yet.
pub fn handle_confirm_transfer_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Enter {
        return run_confirmed_transfer(app);
    }
    if key.code == KeyCode::Esc {
        app.overlay = None;
        return Ok(());
    }

    // Everything past this point only edits the destination field, so
    // borrow `pending` once instead of re-matching `Overlay::ConfirmTransfer`
    // per key (each of the arms below used to do its own `if let`).
    let Some(Overlay::ConfirmTransfer(pending)) = &mut app.overlay else {
        return Ok(());
    };
    pending.destination.apply_key(key);

    Ok(())
}


/// `Enter` on the transfer prompt: copies or moves every source and
/// reloads both panels. Takes `PendingTransfer` by `mem::replace` instead
/// of cloning it. One source: `destination` is the full target path (so
/// it doubles as a rename); several: it's the target directory, each name
/// joined on. A failure is logged and the rest continue.
fn run_confirmed_transfer(app: &mut App) -> Result<()> {
    let Some(Overlay::ConfirmTransfer(pending)) = app.overlay.take() else {
        return Ok(());
    };
    let destination_input = PathBuf::from(pending.destination.text().trim());

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
