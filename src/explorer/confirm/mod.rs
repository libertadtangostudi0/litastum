//! Keys and rendering for the two short-lived file-action prompts: F8's
//! delete (`Overlay::ConfirmDelete`) and F5/F6/`Shift+F6`'s copy/move/
//! rename (`Overlay::ConfirmTransfer`), each over a `Pending*` from
//! `app.rs`.

use std::io;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Overlay};
use crate::command_line::Effect;
use crate::notice::Notice;
use crate::yes_no::{self, Answer};


/// The F8/`Shift+Del` prompt: `Y` sends every entry to the Recycle Bin --
/// or, `permanent`, deletes it for good (directories with all in them, as
/// Far does without asking twice) -- on its own thread (`file_job`);
/// `N`/`Esc` cancel. A failed delete doesn't stop the rest.
pub fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::ConfirmDelete(pending)) = &app.overlay else {
        return Ok(Effect::None);
    };

    let answer = yes_no::answer(key);
    debug!(key = ?crate::logging::LoggedKey(key), ?answer, count = pending.entries.len(), "confirm-delete key");

    match answer {
        Answer::Yes => {
            let Some(Overlay::ConfirmDelete(pending)) = app.overlay.take() else {
                unreachable!("just matched Overlay::ConfirmDelete above");
            };
            // On a thread of its own: a large folder froze the app.
            super::file_job::begin_delete(app, pending.entries, pending.permanent);
        }
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }

    Ok(Effect::None)
}


/// Key handling on the F5/F6 "copy/move to?" prompt: the destination
/// is a standard text field (`TextField::apply_key` -- cursor, word
/// moves, selection, OS clipboard). `Enter` starts the transfer on its
/// own thread (`file_job`), both panels reloading when it's done; `Esc`
/// cancels with nothing touched.
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


/// `Enter` on the transfer prompt: copies or moves every source. One
/// source: `destination` is the full target path (so it doubles as a
/// rename); several: it's the target directory, each name joined on. A
/// failure doesn't stop the rest; the failures show as an error notice.
fn run_confirmed_transfer(app: &mut App) -> Result<()> {
    let Some(Overlay::ConfirmTransfer(pending)) = app.overlay.take() else {
        return Ok(());
    };
    let destination_input = PathBuf::from(pending.destination.text().trim());
    let entries = pending
        .sources
        .iter()
        .map(|source| {
            let destination = if pending.sources.len() == 1 { destination_input.clone() } else { destination_input.join(&source.name) };
            debug!(source = %source.path.display(), destination = %destination.display(), op = ?pending.operation, "confirm-transfer: starting");
            super::file_job::JobEntry { path: source.path.clone(), name: source.name.clone(), to: Some(destination) }
        })
        .collect();
    // On a thread of its own, with progress (`file_job`).
    super::file_job::begin_transfer(app, pending.operation, entries);
    Ok(())
}



/// The error notice for a bulk file operation, `None` if nothing failed:
/// the one failure in full, or a count plus the first one.
pub(super) fn failure_notice(verb: &str, failures: &[(String, io::Error)], total: usize) -> Option<Notice> {
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
