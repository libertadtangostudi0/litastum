#![cfg(test)]

use std::time::{Duration, Instant};

use crate::app::{App, Mode};
use crate::explorer::FindFileState;
use crate::test_support::{test_app, unique_scratch_dir};

use super::super::state::FindFilePhase;

/// Shared by `mod.rs`'s, `typing.rs`'s, and `results.rs`'s own test
/// modules -- `pub(super)` (visible throughout `input` and all of its
/// descendants, which includes those three sibling test modules) rather
/// than duplicating this in each file.
pub(super) fn app_with_find_file(state: FindFileState) -> App {
    let mut app = test_app(unique_scratch_dir("find-file-app"));
    app.mode = Mode::FindFile(state);
    app
}

/// Polls `Mode::FindFile`'s pending background search
/// (`background::poll_pending_find_file_search`) until it leaves
/// `FindFilePhase::Searching` -- `run_search` (triggered by `Enter`)
/// only *starts* a search now, on a real background thread, rather
/// than blocking until it's done the way the old synchronous version
/// did; tests that care about the actual results need to wait for it
/// the same way `main.rs::wait_for_event` does in the real app.
pub(super) fn wait_for_search(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let Mode::FindFile(state) = &app.mode else {
            panic!("expected Mode::FindFile while waiting for a search");
        };
        if state.phase != FindFilePhase::Searching {
            return;
        }
        assert!(Instant::now() < deadline, "search did not finish within the test timeout");
        crate::explorer::poll_pending_find_file_search(app);
    }
}
