use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::app::{App, Mode};

use super::search::{search_cancelable, SearchProgress};
use super::state::FindFilePhase;

/// A Find file search running on a background thread --
/// `FindFileState::pending`'s own `Some` case. Requested directly, after
/// a comparison against real Far Manager's own Find file dialog: a
/// large real search on this app's own single-threaded, synchronous
/// `search::search` used to block the whole UI with nothing to look at,
/// indistinguishable from a hang even though it was just slow -- real
/// Far's own dialog shows live progress and lets `Esc` cancel
/// mid-search instead.
///
/// Mirrors `explorer::image_preview::PendingDecode`'s own shape
/// exactly: a one-shot channel, no generation counter needed to detect
/// a *stale* result, since there's only ever one `PendingSearch` alive
/// in `FindFileState` at a time -- starting a new search or closing the
/// popup simply drops this, and the background thread's own eventual
/// `sender.send(..)` against a receiver nobody's listening on anymore
/// just fails silently (the thread still finishes -- or notices
/// `cancel` and stops -- and exits normally either way, its result just
/// goes nowhere).
pub struct PendingSearch {
    receiver: Receiver<Vec<PathBuf>>,
    /// Live visited/found counters the background thread updates as it
    /// goes -- read directly by `ui/find_file.rs::draw_searching` for
    /// the "Searching... N visited, M found" line, no polling needed
    /// for that part (only the *finished result* needs `poll`).
    pub progress: Arc<SearchProgress>,
    cancel: Arc<AtomicBool>,
    /// When this search was spawned -- `elapsed()` from here, read once
    /// `poll` notices the background thread has finished, becomes
    /// `FindFileState::search_duration`. Also used to show a live
    /// "how long so far" line while still searching.
    pub started: Instant,
}

/// Spawns `search::search_cancelable` on a background thread and
/// returns immediately -- the caller (`input.rs::run_search`) never
/// blocks on the search itself, only on however long it takes to spin
/// up the thread.
pub fn spawn_search(root: PathBuf, query: String, content_query: String) -> PendingSearch {
    let (sender, receiver) = std::sync::mpsc::channel();
    let progress = Arc::new(SearchProgress::default());
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_progress = Arc::clone(&progress);
    let thread_cancel = Arc::clone(&cancel);

    thread::spawn(move || {
        let results = search_cancelable(&root, &query, &content_query, &thread_progress, &thread_cancel);
        let _ = sender.send(results);
    });

    PendingSearch { receiver, progress, cancel, started: Instant::now() }
}

impl PendingSearch {
    /// `Esc` during `FindFilePhase::Searching`: asks the background
    /// thread to stop at its own next check point rather than blocking
    /// the UI until it actually does -- see `search.rs::search_cancelable`'s
    /// own doc comment for how quickly it actually notices. The
    /// eventual `send` against a receiver dropped the moment this whole
    /// `PendingSearch` goes away (the popup closing right after this
    /// call) just fails silently, same as `image_preview.rs`'s own
    /// cancellation-by-replacement already relies on.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Non-blocking check for whether the background search has
    /// finished -- `Some(results)` once it has (the channel is a
    /// one-shot, so this only ever fires once); `None` while still
    /// running. `Some(Vec::new())` on a disconnected channel too (the
    /// background thread panicked) -- same "give up, don't wait
    /// forever" choice `ImagePreviewState::poll` makes for its own
    /// analogous disconnected case.
    fn poll(&self) -> Option<Vec<PathBuf>> {
        match self.receiver.try_recv() {
            Ok(results) => Some(results),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Vec::new()),
        }
    }
}


/// Whether `Mode::FindFile`'s own search is currently running on a
/// background thread -- `main.rs::wait_for_event` polls more often than
/// its own default idle cadence while this is `true`, mirroring
/// `explorer::image_preview::is_image_decode_pending`'s own reasoning
/// exactly: a finished search should be picked up within one short
/// tick, not wait for the next real keyboard/mouse event to happen to
/// wake the main loop up anyway.
pub fn is_find_file_search_pending(app: &App) -> bool {
    matches!(&app.mode, Mode::FindFile(state) if state.pending.is_some())
}

/// Checks whether `Mode::FindFile`'s pending background search has
/// finished, applying the results in place and switching to
/// `FindFilePhase::Results` if so -- see `PendingSearch::poll`. `false`
/// (a no-op) outside `Mode::FindFile` or with nothing pending.
pub fn poll_pending_find_file_search(app: &mut App) -> bool {
    let Mode::FindFile(state) = &mut app.mode else {
        return false;
    };
    let Some(pending) = &state.pending else {
        return false;
    };
    let Some(results) = pending.poll() else {
        return false;
    };

    state.search_duration = Some(pending.started.elapsed());
    state.results_capped = pending.progress.capped.load(Ordering::Relaxed);
    state.results = results;
    state.selected = 0;
    state.phase = FindFilePhase::Results;
    state.pending = None;
    true
}


#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::test_support::unique_scratch_dir;

    #[test]
    fn spawn_search_eventually_reports_the_real_results() {
        let dir = unique_scratch_dir("find-file-background");
        std::fs::write(dir.join("readme.txt"), b"hi").unwrap();

        let pending = spawn_search(dir.clone(), "read".to_string(), String::new());

        let mut results = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while results.is_none() && Instant::now() < deadline {
            results = pending.poll();
        }

        assert_eq!(results, Some(vec![dir.join("readme.txt")]), "the background thread should eventually report the real search results");
    }

    #[test]
    fn cancel_stops_a_search_before_it_finds_everything() {
        let dir = unique_scratch_dir("find-file-background");
        for i in 0..2000 {
            std::fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        let pending = spawn_search(dir.clone(), "file".to_string(), String::new());
        pending.cancel();

        let mut results = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while results.is_none() && Instant::now() < deadline {
            results = pending.poll();
        }

        let results = results.expect("the background thread should still finish (early) and report something, even once cancelled");
        assert!(results.len() < 2000, "a cancelled search shouldn't have had time to find every one of the 2000 files: found {}", results.len());
    }

    #[test]
    fn is_find_file_search_pending_is_false_outside_find_file_mode() {
        let app = crate::test_support::test_app(unique_scratch_dir("find-file-background-app"));
        assert!(!is_find_file_search_pending(&app));
    }

    #[test]
    fn poll_pending_find_file_search_is_a_noop_outside_find_file_mode() {
        let mut app = crate::test_support::test_app(unique_scratch_dir("find-file-background-app"));
        assert!(!poll_pending_find_file_search(&mut app));
    }

    #[test]
    fn poll_pending_find_file_search_applies_results_and_switches_phase_once_finished() {
        let dir = unique_scratch_dir("find-file-background-app");
        std::fs::write(dir.join("readme.txt"), b"hi").unwrap();
        let mut app = crate::test_support::test_app(dir.clone());
        let mut state = super::super::state::FindFileState::new();
        state.query = "read".to_string();
        state.phase = FindFilePhase::Searching;
        state.pending = Some(spawn_search(dir.clone(), "read".to_string(), String::new()));
        app.mode = Mode::FindFile(state);

        assert!(is_find_file_search_pending(&app), "should start pending");
        let deadline = Instant::now() + Duration::from_secs(5);
        while is_find_file_search_pending(&app) && Instant::now() < deadline {
            poll_pending_find_file_search(&mut app);
        }

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results, vec![dir.join("readme.txt")]);
        assert!(state.pending.is_none());
        assert!(state.search_duration.is_some());
    }
}
