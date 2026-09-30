//! Keys and rendering for the two short-lived file-action prompts: F8's
//! delete (`Overlay::ConfirmDelete`) and F5/F6/`Shift+F6`'s copy/move/
//! rename (`Overlay::ConfirmTransfer`), each over a `Pending*` from
//! `app.rs`.

use std::fs;
use std::io;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Overlay, TransferOp};
use crate::command_line::Effect;
use crate::notice::Notice;
use crate::yes_no::{self, Answer};
use super::fs_ops;


/// The F8 prompt: `Y` deletes every entry (directories recursively, as
/// Far does without asking twice) and reloads; `N`/`Esc` cancel. A failed
/// delete doesn't stop the rest; the failures show as an error notice.
pub fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::ConfirmDelete(pending)) = &app.overlay else {
        return Ok(Effect::None);
    };

    let answer = yes_no::answer(key);
    debug!(?key, ?answer, count = pending.entries.len(), "confirm-delete key");

    match answer {
        Answer::Yes => {
            let Some(Overlay::ConfirmDelete(pending)) = app.overlay.take() else {
                unreachable!("just matched Overlay::ConfirmDelete above");
            };
            let mut failures = Vec::new();
            for entry in &pending.entries {
                let result = if entry.is_dir {
                    fs::remove_dir_all(&entry.path)
                } else {
                    fs::remove_file(&entry.path)
                };
                if let Err(err) = result {
                    debug!(path = %entry.path.display(), %err, "delete failed");
                    failures.push((entry.name.clone(), err));
                }
            }
            app.notice = failure_notice("Delete", &failures, pending.entries.len());
            app.active_panel().reload()?;
        }
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }

    Ok(Effect::None)
}


/// Key handling on the F5/F6 "copy/move to?" prompt: the destination
/// is a standard text field (`TextField::apply_key` -- cursor, word
/// moves, selection, OS clipboard). `Enter` performs the transfer (`fs_ops::copy_entry`/
/// `move_entry`) and reloads *both* panels (the destination side
/// always needs it, and a move also changes the source side); `Esc`
/// cancels with nothing touched. Failures show as an error notice
/// (`run_confirmed_transfer`).
pub fn handle_confirm_transfer_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if key.code == KeyCode::Enter {
        run_confirmed_transfer(app)?;
        return Ok(Effect::None);
    }
    if key.code == KeyCode::Esc {
        app.overlay = None;
        return Ok(Effect::None);
    }

    // Everything past this point only edits the destination field, so
    // borrow `pending` once instead of re-matching `Overlay::ConfirmTransfer`
    // per key (each of the arms below used to do its own `if let`).
    let Some(Overlay::ConfirmTransfer(pending)) = &mut app.overlay else {
        return Ok(Effect::None);
    };
    pending.destination.apply_key(key);

    Ok(Effect::None)
}


/// `Enter` on the transfer prompt: copies or moves every source and
/// reloads both panels. Takes `PendingTransfer` by `mem::replace` instead
/// of cloning it. One source: `destination` is the full target path (so
/// it doubles as a rename); several: it's the target directory, each name
/// joined on. A failure doesn't stop the rest; the failures show as an
/// error notice.
fn run_confirmed_transfer(app: &mut App) -> Result<()> {
    let Some(Overlay::ConfirmTransfer(pending)) = app.overlay.take() else {
        return Ok(());
    };
    let destination_input = PathBuf::from(pending.destination.text().trim());

    let mut failures = Vec::new();
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
            failures.push((source.name.clone(), err));
        }
    }
    let verb = match pending.operation {
        TransferOp::Copy => "Copy",
        TransferOp::Move => "Move",
    };
    app.notice = failure_notice(verb, &failures, pending.sources.len());

    for panel in &mut app.panels {
        panel.reload()?;
    }
    Ok(())
}



/// The error notice for a bulk file operation, `None` if nothing failed:
/// the one failure in full, or a count plus the first one.
fn failure_notice(verb: &str, failures: &[(String, io::Error)], total: usize) -> Option<Notice> {
    let (name, err) = failures.first()?;
    let text = if failures.len() == 1 {
        format!("{verb} failed: {name}: {err}")
    } else {
        format!("{verb} failed for {} of {total} entries; first: {name}: {err}", failures.len())
    };
    Some(Notice::error(text))
}


#[cfg(test)]
mod tests;
