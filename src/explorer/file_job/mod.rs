//! Copy, move and delete run on a thread of their own (reported: a large
//! folder froze the app), one at a time. A toast shows how far along it
//! is (`FileJob::message`); `Esc` on the panels asks to stop it
//! (`Overlay::ConfirmStopJob`): what's done stays, the file being copied
//! is removed. When it ends, both panels reload and a failure shows as a
//! notice. The Recycle Bin is the shell's `IFileOperation`
//! (`recycle_bin`), its progress in the shell's own units.

mod copy_file;
mod copy_tree;
#[cfg(all(windows, not(test)))]
mod recycle_bin;
mod work;

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::{debug, warn};

use crate::app::{App, DeleteEntry, Overlay, TransferOp};
use crate::command_line::Effect;
use crate::notice::Notice;
use crate::yes_no::{self, Answer};


/// How far a job is, shared with its thread. The totals are counted
/// first (`counted` then set), so a percentage only shows after that.
#[derive(Default)]
pub(super) struct Progress {
    total_bytes: AtomicU64,
    done_bytes: AtomicU64,
    total_items: AtomicU64,
    done_items: AtomicU64,
    counted: AtomicBool,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    /// To the Recycle Bin.
    Recycle,
    /// For good.
    Delete,
}


impl JobKind {
    fn verb(self) -> &'static str {
        match self {
            JobKind::Copy => "Copying",
            JobKind::Move => "Moving",
            JobKind::Recycle | JobKind::Delete => "Deleting",
        }
    }

    /// For a failure notice.
    fn noun(self) -> &'static str {
        match self {
            JobKind::Copy => "Copy",
            JobKind::Move => "Move",
            JobKind::Recycle | JobKind::Delete => "Delete",
        }
    }
}


/// How often a running job's toast is redrawn: nothing else redraws it
/// while no key is pressed (reported: the progress moved only on a key).
const REFRESH_INTERVAL: Duration = Duration::from_millis(100);


/// One entry of a job: what to act on, and where to (copy/move).
pub(super) struct JobEntry {
    pub path: PathBuf,
    pub name: String,
    pub to: Option<PathBuf>,
}


/// A copy, move or delete in progress.
pub struct FileJob {
    kind: JobKind,
    /// What the toast names: the entry, or how many.
    label: String,
    total: usize,
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    started: Instant,
    /// When the toast is next redrawn (`poll_pending_file_job`).
    next_refresh: Instant,
    done: Receiver<Vec<(String, io::Error)>>,
}


impl FileJob {
    fn start(kind: JobKind, entries: Vec<JobEntry>) -> Self {
        let label = match entries.as_slice() {
            [single] => single.name.clone(),
            many => format!("{} entries", many.len()),
        };
        let total = entries.len();
        let progress = Arc::new(Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, done) = mpsc::channel();
        let (thread_progress, thread_cancel) = (Arc::clone(&progress), Arc::clone(&cancel));
        std::thread::spawn(move || {
            let failures = run(kind, &entries, &thread_progress, &thread_cancel);
            // The app may have quit meanwhile; nothing to tell then.
            let _ = sender.send(failures);
        });
        let started = Instant::now();
        Self { kind, label, total, progress, cancel, started, next_refresh: started + REFRESH_INTERVAL, done }
    }


    /// The toast's text: how far along it is, and how to stop it.
    pub fn message(&self) -> String {
        let verb = self.kind.verb();
        let progress = &self.progress;
        let stop = " · Esc stops";
        if self.cancel.load(Ordering::Relaxed) {
            return format!("{verb} {}: stopping...", self.label);
        }
        if !progress.counted.load(Ordering::Relaxed) {
            // The shell says nothing until it has looked the tree over.
            if self.kind == JobKind::Recycle {
                let seconds = self.started.elapsed().as_secs();
                return format!("{verb} {}... {}:{:02}{stop}", self.label, seconds / 60, seconds % 60);
            }
            return format!("{verb} {}: counting...{stop}", self.label);
        }
        let (done, total) = match self.kind {
            JobKind::Delete | JobKind::Recycle => (progress.done_items.load(Ordering::Relaxed), progress.total_items.load(Ordering::Relaxed)),
            _ => (progress.done_bytes.load(Ordering::Relaxed), progress.total_bytes.load(Ordering::Relaxed)),
        };
        let percent = (done.min(total) * 100).checked_div(total).unwrap_or(100);
        let amounts = match self.kind {
            // The shell's units mean nothing to show: just the percentage.
            JobKind::Recycle => return format!("{verb} {}: {percent}%{stop}", self.label),
            JobKind::Delete => format!("{done} of {total} entries"),
            _ => format!("{} of {}", super::format_bytes(done), super::format_bytes(total)),
        };
        format!("{verb} {}: {percent}% ({amounts}){stop}", self.label)
    }


    /// How much is done, `0.0..=1.0` -- `None` until the totals are
    /// counted, when the bar only shows it's busy.
    pub fn fraction(&self) -> Option<f64> {
        let progress = &self.progress;
        if !progress.counted.load(Ordering::Relaxed) {
            return None;
        }
        let (done, total) = match self.kind {
            JobKind::Delete | JobKind::Recycle => (progress.done_items.load(Ordering::Relaxed), progress.total_items.load(Ordering::Relaxed)),
            _ => (progress.done_bytes.load(Ordering::Relaxed), progress.total_bytes.load(Ordering::Relaxed)),
        };
        Some(if total == 0 { 1.0 } else { done.min(total) as f64 / total as f64 })
    }


    /// How long it has run -- the busy bar moves with it.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }


    pub fn kind(&self) -> JobKind {
        self.kind
    }


    /// Asks the job to stop; it does at the next file, or mid-file for a
    /// copy or the Recycle Bin.
    fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }


    /// Stops it and waits until it has -- at exit, so it isn't cut off
    /// half-way.
    pub fn finish_at_exit(self) {
        self.stop();
        let _ = self.done.recv();
    }
}


/// The job's thread: counts first, then works through the entries,
/// stopping at `cancel`. The Recycle Bin goes to the shell in one
/// operation, which counts on its own; in tests, never the real Recycle
/// Bin of whoever runs them, and outside Windows, it's a delete for good.
fn run(kind: JobKind, entries: &[JobEntry], progress: &Arc<Progress>, cancel: &Arc<AtomicBool>) -> Vec<(String, io::Error)> {
    #[cfg(all(windows, not(test)))]
    if kind == JobKind::Recycle {
        return recycle_bin::send(entries, progress, cancel);
    }
    // A move on one drive is a rename, done at once: tried first, so only
    // what has to be copied is counted -- counting a whole tree just to
    // rename it took as long as the walk.
    let left: Vec<&JobEntry> = match kind {
        JobKind::Move => entries.iter().filter(|entry| !entry.to.as_ref().is_some_and(|to| std::fs::rename(&entry.path, to).is_ok())).collect(),
        _ => entries.iter().collect(),
    };
    for entry in &left {
        work::count(&entry.path, progress, cancel);
    }
    progress.counted.store(true, Ordering::Relaxed);
    let mut failures = Vec::new();
    for entry in left {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let result = match (kind, &entry.to) {
            (JobKind::Copy, Some(to)) => copy_tree::copy_tree(&entry.path, to, progress, cancel),
            (JobKind::Move, Some(to)) => move_by_copy(&entry.path, to, progress, cancel),
            (JobKind::Recycle | JobKind::Delete, _) => work::delete_tree(&entry.path, progress, cancel),
            (_, None) => Err(io::Error::other("no destination")),
        };
        match result {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::Interrupted => break,
            Err(err) => {
                debug!(path = %entry.path.display(), %err, ?kind, "file job: an entry failed");
                failures.push((entry.name.clone(), err));
            }
        }
    }
    failures
}


/// A move a rename couldn't do (another drive): a copy, and the source
/// deleted once the copy is whole.
fn move_by_copy(from: &std::path::Path, to: &std::path::Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    copy_tree::copy_tree(from, to, progress, cancel)?;
    let removed = Progress::default();
    work::delete_tree(from, &removed, &AtomicBool::new(false))
}


pub fn is_file_job_pending(app: &App) -> bool {
    app.file_job.is_some()
}


/// A finished job: the panels reload, a failure becomes a notice, a
/// stopped one says so. `true` if it finished, or if a running one's
/// toast is due a redraw (`REFRESH_INTERVAL`).
pub fn poll_pending_file_job(app: &mut App) -> bool {
    let Some(job) = &mut app.file_job else {
        return false;
    };
    let failures = match job.done.try_recv() {
        Ok(failures) => failures,
        Err(TryRecvError::Empty) => {
            let now = Instant::now();
            if now < job.next_refresh {
                return false;
            }
            job.next_refresh = now + REFRESH_INTERVAL;
            return true;
        }
        Err(TryRecvError::Disconnected) => vec![(job.label.clone(), io::Error::other("the operation stopped"))],
    };
    let (kind, total, stopped) = (job.kind, job.total, job.cancel.load(Ordering::Relaxed));
    let label = job.label.clone();
    app.file_job = None;
    if matches!(app.overlay, Some(Overlay::ConfirmStopJob)) {
        app.overlay = None;
    }
    app.notice = match super::confirm::failure_notice(kind.noun(), &failures, total) {
        Some(notice) => Some(notice),
        None if stopped => Some(Notice::info(format!("{} {label}: stopped; what was done stays", kind.noun()))),
        None => None,
    };
    for panel in &mut app.panels {
        if let Err(err) = panel.reload() {
            warn!(%err, "reload after a file operation failed");
        }
    }
    true
}


/// Starts a job, unless one still runs: one at a time, so the panels
/// reload once it's all done.
pub(super) fn begin(app: &mut App, kind: JobKind, entries: Vec<JobEntry>) {
    if let Some(running) = &app.file_job {
        app.notice = Some(Notice::error(format!("Still {} {}; try again when it's done", running.kind.verb().to_lowercase(), running.label)));
        return;
    }
    app.file_job = Some(FileJob::start(kind, entries));
}


/// A confirmed delete: to the Recycle Bin, or for good.
pub(super) fn begin_delete(app: &mut App, entries: Vec<DeleteEntry>, permanent: bool) {
    let kind = if permanent { JobKind::Delete } else { JobKind::Recycle };
    let entries = entries.into_iter().map(|entry| JobEntry { path: entry.path, name: entry.name, to: None }).collect();
    begin(app, kind, entries);
}


/// A confirmed copy or move -- refused if it would go into itself: a
/// folder copied into its own subfolder copied itself over and over until
/// the disk was full.
pub(super) fn begin_transfer(app: &mut App, operation: TransferOp, entries: Vec<JobEntry>) {
    let kind = match operation {
        TransferOp::Copy => JobKind::Copy,
        TransferOp::Move => JobKind::Move,
    };
    if let Some(entry) = entries.iter().find(|entry| entry.to.as_ref().is_some_and(|to| work::is_within(to, &entry.path))) {
        app.notice = Some(Notice::error(format!("Can't {} {} into itself", kind.noun().to_lowercase(), entry.name)));
        return;
    }
    begin(app, kind, entries);
}


/// `Esc` on the panels while a job runs: asks first. `true` if it asked.
pub(crate) fn ask_to_stop(app: &mut App) -> bool {
    match &app.file_job {
        Some(job) if !job.cancel.load(Ordering::Relaxed) => {
            app.overlay = Some(Overlay::ConfirmStopJob);
            true
        }
        _ => false,
    }
}


/// `Y` stops the job, `N`/`Esc` let it go on.
pub fn handle_confirm_stop_job_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    match yes_no::answer(key) {
        Answer::Yes => {
            app.overlay = None;
            if let Some(job) = &app.file_job {
                debug!(kind = ?job.kind, "file job: stopping");
                job.stop();
            }
        }
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }
    Ok(Effect::None)
}


/// A job that runs until its `Sender` says it's done -- tests only, as a
/// large folder seems to.
#[cfg(test)]
pub(crate) fn running_for_test(kind: JobKind, label: &str) -> (FileJob, mpsc::Sender<Vec<(String, io::Error)>>) {
    let (sender, done) = mpsc::channel();
    let now = Instant::now();
    let job = FileJob { kind, label: label.to_string(), total: 1, progress: Arc::new(Progress::default()), cancel: Arc::new(AtomicBool::new(false)), started: now, next_refresh: now + REFRESH_INTERVAL, done };
    (job, sender)
}


/// The bytes a job has done, counted, as its thread would report them --
/// tests only.
#[cfg(test)]
pub(crate) fn report_for_test(job: &FileJob, done_bytes: u64, total_bytes: u64) {
    job.progress.total_bytes.store(total_bytes, Ordering::Relaxed);
    job.progress.done_bytes.store(done_bytes, Ordering::Relaxed);
    job.progress.counted.store(true, Ordering::Relaxed);
}


/// Waits for a job to finish -- tests only, where it's quick.
#[cfg(test)]
pub(crate) fn wait_for_file_job(app: &mut App) {
    while app.file_job.is_some() {
        if !poll_pending_file_job(app) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}


#[cfg(test)]
mod tests;
