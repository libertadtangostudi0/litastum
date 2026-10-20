//! A confirmed delete runs on its own thread (reported: a large folder
//! froze the app while the shell sent it to the Recycle Bin). While it
//! runs a toast says so (`ui::draw`); when it's done, the panels reload
//! and a failure shows as a notice.

use std::io;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use tracing::{debug, warn};

use crate::app::{App, DeleteEntry};
use crate::notice::Notice;


/// A delete in progress.
pub struct Deletion {
    /// What the toast names: the entry, or how many.
    label: String,
    total: usize,
    done: Receiver<Vec<(String, io::Error)>>,
}


impl Deletion {
    /// Deletes `entries` -- to the Recycle Bin, or for good -- on a thread
    /// of its own.
    pub fn start(entries: Vec<DeleteEntry>, permanent: bool) -> Self {
        let label = match entries.as_slice() {
            [single] => single.name.clone(),
            many => format!("{} entries", many.len()),
        };
        let total = entries.len();
        let (sender, done) = mpsc::channel();
        std::thread::spawn(move || {
            let mut failures = Vec::new();
            for entry in &entries {
                let result = if permanent { super::recycle::delete_for_good(&entry.path, entry.is_dir) } else { super::recycle::to_recycle_bin(&entry.path, entry.is_dir) };
                if let Err(err) = result {
                    debug!(path = %entry.path.display(), %err, "delete failed");
                    failures.push((entry.name.clone(), err));
                }
            }
            // The app may have quit meanwhile; nothing to tell then.
            let _ = sender.send(failures);
        });
        Self { label, total, done }
    }


    /// Blocks until it's done -- at exit, so it isn't cut off half-way.
    pub fn wait(self) {
        let _ = self.done.recv();
    }


    /// The toast's text while it runs.
    pub fn message(&self) -> String {
        format!("Deleting {}...", self.label)
    }
}


pub fn is_deletion_pending(app: &App) -> bool {
    app.deletion.is_some()
}


/// A finished delete: the panels reload, a failure becomes a notice.
/// `true` if it finished.
pub fn poll_pending_deletion(app: &mut App) -> bool {
    let Some(deletion) = &app.deletion else {
        return false;
    };
    let failures = match deletion.done.try_recv() {
        Ok(failures) => failures,
        Err(TryRecvError::Empty) => return false,
        Err(TryRecvError::Disconnected) => vec![(deletion.label.clone(), io::Error::other("the delete stopped"))],
    };
    let total = deletion.total;
    app.deletion = None;
    if let Some(notice) = super::confirm::failure_notice("Delete", &failures, total) {
        app.notice = Some(notice);
    }
    for panel in &mut app.panels {
        if let Err(err) = panel.reload() {
            warn!(%err, "reload after a delete failed");
        }
    }
    true
}


/// Starts deleting, unless a delete still runs: one at a time, so the
/// panels reload once it's all done.
pub(super) fn begin(app: &mut App, entries: Vec<DeleteEntry>, permanent: bool) {
    if let Some(running) = &app.deletion {
        app.notice = Some(Notice::error(format!("Still deleting {}; try again when it's done", running.label)));
        return;
    }
    app.deletion = Some(Deletion::start(entries, permanent));
}


/// Waits for a delete to finish -- tests only, where it's quick.
#[cfg(test)]
pub(crate) fn wait_for_deletion(app: &mut App) {
    while app.deletion.is_some() {
        if !poll_pending_deletion(app) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    /// One that never finishes, as a large folder seems to.
    fn running(label: &str) -> (Deletion, mpsc::Sender<Vec<(String, io::Error)>>) {
        let (sender, done) = mpsc::channel();
        (Deletion { label: label.to_string(), total: 1, done }, sender)
    }

    /// Reported: a large folder froze the app. The delete runs on its own;
    /// the app goes on, the toast saying what's being deleted.
    #[test]
    fn a_running_delete_leaves_the_app_free_and_says_so() {
        let mut app = test_app(unique_scratch_dir("deletion"));
        let (deletion, sender) = running("big-folder");
        app.deletion = Some(deletion);

        assert!(!poll_pending_deletion(&mut app), "not done yet");
        assert_eq!(app.deletion.as_ref().map(Deletion::message).as_deref(), Some("Deleting big-folder..."));

        sender.send(Vec::new()).unwrap();
        assert!(poll_pending_deletion(&mut app));
        assert!(app.deletion.is_none());
        assert!(app.notice.is_none(), "nothing failed");
    }

    #[test]
    fn a_second_delete_waits_for_the_first() {
        let dir = unique_scratch_dir("deletion");
        let file = dir.join("keep.txt");
        std::fs::write(&file, "x").unwrap();
        let mut app = test_app(dir);
        let (deletion, _sender) = running("big-folder");
        app.deletion = Some(deletion);

        begin(&mut app, vec![DeleteEntry { path: file.clone(), name: "keep.txt".into(), is_dir: false, size: 1 }], true);

        assert!(file.exists(), "not started");
        assert!(app.notice.as_ref().is_some_and(|notice| notice.text.contains("Still deleting big-folder")));
    }
}
