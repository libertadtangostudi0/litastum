use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::SearchProgress;

mod scan;

/// Checks each candidate's content for `needle_lower` on
/// `available_parallelism()` threads, stopping at `max_results` or on
/// `cancel`. `progress.visited` keeps counting, now meaning candidates
/// checked.
///
/// - Work comes from a shared `next_index` counter, not a fixed slice per
///   thread: file sizes vary wildly, and a thread with the big ones kept
///   working long after the rest went idle.
/// - "Enough found?" is an atomic load of `progress.found`; the `Mutex`
///   is only taken to push a match. Checking the list's length under the
///   lock on every iteration contended on the common no-match path.
///
/// History: docs/history/find-file-search.md.
pub(super) fn content_filter_in_parallel(candidates: Vec<PathBuf>, needle_lower: &str, max_results: usize, progress: &SearchProgress, cancel: &AtomicBool) -> Vec<PathBuf> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1).min(candidates.len());
    let next_index = AtomicUsize::new(0);
    let found: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for _ in 0..threads {
            let next_index = &next_index;
            let found = &found;
            let candidates = &candidates;
            scope.spawn(move || loop {
                if cancel.load(Ordering::Relaxed) || progress.found.load(Ordering::Relaxed) >= max_results {
                    return;
                }
                let index = next_index.fetch_add(1, Ordering::Relaxed);
                let Some(path) = candidates.get(index) else {
                    return; // no more candidates left to claim
                };

                let matched = scan::file_contains(path, needle_lower);
                progress.visited.fetch_add(1, Ordering::Relaxed);
                if matched {
                    let mut found = found.lock().unwrap();
                    if found.len() < max_results {
                        found.push(path.clone());
                        progress.found.store(found.len(), Ordering::Relaxed);
                        if found.len() >= max_results {
                            progress.capped.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });

    found.into_inner().unwrap()
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("find-file-search-content")
    }

    /// Regression coverage for the real report: a content-query search
    /// that happened to hit exactly `max_results` should be flagged as
    /// capped, same as the name-only walk (`walk.rs`'s own coverage) --
    /// otherwise this phase's own results cap goes just as unreported.
    #[test]
    fn content_filter_in_parallel_sets_capped_when_the_result_cap_is_hit() {
        let dir = scratch_dir();
        let candidates: Vec<PathBuf> = (0..10)
            .map(|i| {
                let path = dir.join(format!("file_{i}.txt"));
                fs::write(&path, b"needle").unwrap();
                path
            })
            .collect();

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(false);
        let results = content_filter_in_parallel(candidates, "needle", 5, &progress, &cancel);

        assert_eq!(results.len(), 5);
        assert!(progress.capped.load(Ordering::Relaxed), "hitting max_results should mark the search as capped");
    }

    #[test]
    fn content_filter_in_parallel_does_not_set_capped_when_nothing_was_truncated() {
        let dir = scratch_dir();
        let candidates: Vec<PathBuf> = (0..3)
            .map(|i| {
                let path = dir.join(format!("file_{i}.txt"));
                fs::write(&path, b"needle").unwrap();
                path
            })
            .collect();

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(false);
        content_filter_in_parallel(candidates, "needle", 50, &progress, &cancel);

        assert!(!progress.capped.load(Ordering::Relaxed), "a search that genuinely finished should not claim to be capped");
    }

    /// Regression coverage for the shared-`next_index` work distribution:
    /// every matching candidate should still be found exactly once,
    /// across a candidate list large and uneven enough (most files tiny,
    /// a few deliberately large) to actually exercise more than one
    /// thread claiming more than its own "fair share" of the work.
    #[test]
    fn content_filter_in_parallel_finds_every_match_across_uneven_candidates() {
        let dir = scratch_dir();
        let mut candidates = Vec::new();
        for i in 0..100 {
            let path = dir.join(format!("small_{i}.txt"));
            fs::write(&path, b"nothing interesting here").unwrap();
            candidates.push(path);
        }
        for i in 0..5 {
            let path = dir.join(format!("large_{i}.txt"));
            let mut content = vec![b'x'; 200_000];
            content.extend_from_slice(b"needle");
            fs::write(&path, content).unwrap();
            candidates.push(path);
        }

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(false);
        let mut results = content_filter_in_parallel(candidates, "needle", 100, &progress, &cancel);
        results.sort();

        let mut expected: Vec<PathBuf> = (0..5).map(|i| dir.join(format!("large_{i}.txt"))).collect();
        expected.sort();
        assert_eq!(results, expected);
    }

    /// Same cancellation guarantee `search_cancelable_stops_once_cancelled`
    /// (`search/mod.rs`) checks end-to-end, but for the parallel
    /// content-check phase specifically -- that end-to-end test can't
    /// exercise this path on its own, since a search cancelled before it
    /// starts never collects any candidates for
    /// `content_filter_in_parallel` to even see. Calls it directly with
    /// a real, non-empty candidate list instead.
    #[test]
    fn content_filter_in_parallel_stops_once_cancelled() {
        let dir = scratch_dir();
        let candidates: Vec<PathBuf> = (0..50)
            .map(|i| {
                let path = dir.join(format!("file_{i}.txt"));
                fs::write(&path, b"needle").unwrap();
                path
            })
            .collect();

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(true); // already cancelled before the check even starts
        let results = content_filter_in_parallel(candidates, "needle", 50, &progress, &cancel);

        assert!(results.is_empty(), "a content check cancelled before it starts should find nothing");
    }
}
