//! The panels' last directories, for the next start (requested): kept in
//! memory as they change and written to `config.json` (`left_panel_path`,
//! `right_panel_path`) 30 s after a change -- insurance against a crash --
//! when litastum exits, and when its window or tab is closed
//! (`CTRL_CLOSE_EVENT`). Not on every change: navigating wrote the file
//! once per directory.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a change waits to be written. Closing and exiting write at
/// once, so this only bounds what a crash loses.
pub const SAVE_DELAY: Duration = Duration::from_secs(30);

/// The panels' directories now, for the close handler, which runs on a
/// thread of its own while the event loop may be anywhere.
static LATEST: Mutex<Option<[PathBuf; 2]>> = Mutex::new(None);


/// What's saved, what's current, and since when a change has waited.
#[derive(Debug, Default)]
pub struct LastPaths {
    saved: Option<[PathBuf; 2]>,
    current: Option<[PathBuf; 2]>,
    changed_since: Option<Instant>,
}

impl LastPaths {
    /// Starts with `current` as saved -- what was just opened.
    pub fn new(current: [PathBuf; 2]) -> Self {
        *LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(current.clone());
        Self { saved: Some(current.clone()), current: Some(current), changed_since: None }
    }

    /// The panels' directories now, at `now`: a change starts the wait (a
    /// further change doesn't restart it, so a crash loses at most
    /// `SAVE_DELAY`); back where saved, nothing waits.
    pub fn note(&mut self, current: [PathBuf; 2], now: Instant) {
        if self.saved.as_ref() == Some(&current) {
            self.changed_since = None;
        } else if self.changed_since.is_none() {
            self.changed_since = Some(now);
        }
        if self.current.as_ref() != Some(&current) {
            *LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(current.clone());
            self.current = Some(current);
        }
    }

    /// Whether a change waits to be written.
    pub fn pending(&self) -> bool {
        self.changed_since.is_some()
    }

    /// The directories to write once the wait is over; they count as
    /// saved from here.
    pub fn take_due(&mut self, now: Instant) -> Option<[PathBuf; 2]> {
        let since = self.changed_since?;
        if now.duration_since(since) < SAVE_DELAY {
            return None;
        }
        self.take_pending()
    }

    /// The directories to write now (at exit), if changed since saved.
    pub fn take_pending(&mut self) -> Option<[PathBuf; 2]> {
        self.changed_since.take()?;
        let current = self.current.clone()?;
        self.saved = Some(current.clone());
        Some(current)
    }
}


/// Writes `paths` to `config.json` (best effort).
pub fn save(paths: &[PathBuf; 2]) {
    crate::theming::config::save_panel_paths(&paths[0], &paths[1]);
}


/// Writes the panels' latest directories when the console is closed --
/// litastum's window or tab, Windows Terminal's tab, a logoff -- which
/// ends the process without it reaching its own exit. Synchronous, in the
/// handler: Windows ends the process as soon as the handler returns.
#[cfg(windows)]
pub fn install_close_handler() {
    use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT};

    unsafe extern "system" fn on_console_event(event: u32) -> i32 {
        if matches!(event, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
            if let Some(paths) = LATEST.lock().ok().and_then(|latest| latest.clone()) {
                save(&paths);
            }
        }
        // Not handled: the next handler, or the default (ending the
        // process), carries on.
        0
    }

    // SAFETY: registers a plain function; the system calls it on a thread
    // of its own.
    unsafe {
        SetConsoleCtrlHandler(Some(on_console_event), 1);
    }
}

#[cfg(not(windows))]
pub fn install_close_handler() {}


#[cfg(test)]
mod tests {
    use super::*;

    fn paths(left: &str, right: &str) -> [PathBuf; 2] {
        [PathBuf::from(left), PathBuf::from(right)]
    }

    /// Requested: not a write per directory -- one, 30 s after a change.
    #[test]
    fn a_change_is_due_after_the_delay_however_many_follow() {
        let start = Instant::now();
        let mut last = LastPaths::new(paths("a", "b"));
        assert!(!last.pending());

        last.note(paths("a1", "b"), start);
        last.note(paths("a2", "b"), start + Duration::from_secs(20));
        assert!(last.pending());
        assert_eq!(last.take_due(start + Duration::from_secs(25)), None, "not yet");

        assert_eq!(last.take_due(start + SAVE_DELAY), Some(paths("a2", "b")), "30 s after the first change: the latest");
        assert!(!last.pending());
        last.note(paths("a2", "b"), start + Duration::from_secs(40));
        assert!(!last.pending(), "nothing new");
    }

    #[test]
    fn going_back_where_saved_cancels_the_wait() {
        let start = Instant::now();
        let mut last = LastPaths::new(paths("a", "b"));
        last.note(paths("x", "b"), start);
        last.note(paths("a", "b"), start + Duration::from_secs(1));

        assert!(!last.pending());
        assert_eq!(last.take_pending(), None);
    }

    #[test]
    fn exiting_writes_a_pending_change_at_once() {
        let mut last = LastPaths::new(paths("a", "b"));
        last.note(paths("a", "c"), Instant::now());

        assert_eq!(last.take_pending(), Some(paths("a", "c")));
        assert_eq!(last.take_pending(), None, "written: nothing left");
    }
}
