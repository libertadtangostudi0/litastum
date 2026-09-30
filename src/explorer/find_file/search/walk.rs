use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};

use crate::explorer::is_vcs_dir_name;

use super::matching::{matches_query, ParsedQuery};
use super::SearchProgress;

/// Every real entry under `root` minus VCS metadata directories: all of
/// `ignore`'s own filters (`.gitignore`, hidden, `.ignore`) are off -- a
/// gitignored file is still a real file to find. Threads scale with
/// `available_parallelism()` instead of `ignore`'s cap of 12, like the
/// content pool.
fn build_walker(root: &Path) -> WalkBuilder {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let mut builder = WalkBuilder::new(root);
    builder.standard_filters(false);
    builder.threads(threads);
    builder.filter_entry(|entry| !(entry.file_type().is_some_and(|file_type| file_type.is_dir()) && is_vcs_dir_name(&entry.file_name().to_string_lossy())));
    builder
}

/// Walks `root` in parallel, returning every entry (file or directory)
/// whose name matches, up to `max_results`, visiting at most
/// `max_visited`. The root itself is never matched. `WalkState::Quit`
/// stops threads "as soon as possible", so a few entries past a cap may
/// still be visited -- the caps are backstops, not exact bounds.
pub(super) fn matched_names(root: &Path, query_lower: &str, progress: &SearchProgress, cancel: &AtomicBool, max_results: usize, max_visited: usize) -> Vec<PathBuf> {
    let results: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    // Parsed once for the whole walk, not once per entry -- see
    // `ParsedQuery`'s own doc comment.
    let query = ParsedQuery::new(query_lower);

    build_walker(root).build_parallel().run(|| {
        Box::new(|entry_result| {
            if cancel.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            let Ok(entry) = entry_result else {
                return WalkState::Continue;
            };
            if entry.depth() == 0 {
                return WalkState::Continue;
            }

            let visited = progress.visited.fetch_add(1, Ordering::Relaxed) + 1;
            if visited >= max_visited {
                progress.capped.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }

            // `matches_query` itself owns case-folding now (usually
            // without allocating at all, for a plain ASCII name/query)
            // -- see its own doc comment. `to_string_lossy()` alone
            // typically borrows rather than allocates too (a real
            // filename is virtually always already valid Unicode), so
            // the common case now visits an entry with zero allocation
            // on this line.
            let name = entry.file_name().to_string_lossy();
            if matches_query(&name, &query) {
                let mut results = results.lock().unwrap();
                if results.len() < max_results {
                    results.push(entry.into_path());
                    progress.found.store(results.len(), Ordering::Relaxed);
                }
                if results.len() >= max_results {
                    progress.capped.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }
            }

            WalkState::Continue
        })
    });

    results.into_inner().unwrap()
}

/// Same parallel walk as `matched_names`, but for the content-query
/// path: collects every *file* (never a directory — see `search/mod.rs`'s
/// own doc comment for why) whose name matches `query_lower`, uncapped
/// by a result limit (only `max_visited` bounds the walk itself) -- the
/// content check that decides which of these actually become results
/// runs as a separate, parallel pass in `content.rs::content_filter_in_parallel`.
pub(super) fn matched_files(root: &Path, query_lower: &str, progress: &SearchProgress, cancel: &AtomicBool, max_visited: usize) -> Vec<PathBuf> {
    let candidates: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    // Parsed once for the whole walk, not once per entry -- see
    // `ParsedQuery`'s own doc comment.
    let query = ParsedQuery::new(query_lower);

    build_walker(root).build_parallel().run(|| {
        Box::new(|entry_result| {
            if cancel.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            let Ok(entry) = entry_result else {
                return WalkState::Continue;
            };
            if entry.depth() == 0 {
                return WalkState::Continue;
            }

            let visited = progress.visited.fetch_add(1, Ordering::Relaxed) + 1;
            if visited >= max_visited {
                progress.capped.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }

            let is_dir = entry.file_type().is_some_and(|file_type| file_type.is_dir());
            if !is_dir {
                let name = entry.file_name().to_string_lossy();
                if matches_query(&name, &query) {
                    candidates.lock().unwrap().push(entry.into_path());
                }
            }

            WalkState::Continue
        })
    });

    candidates.into_inner().unwrap()
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("find-file-search-walk")
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// Confirms the rewritten parallel walk still finds matches spread
    /// across many different subdirectories -- not just one, since a
    /// broken work-distribution (a directory's own subtree silently
    /// never handed to any worker) would still pass a single-directory
    /// test.
    #[test]
    fn matched_names_finds_entries_across_many_subdirectories() {
        let dir = scratch_dir();
        for i in 0..40 {
            let sub = dir.join(format!("dir_{i}"));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("target.txt"), b"hi").unwrap();
        }

        let progress = SearchProgress::default();
        let mut results = matched_names(&dir, "target", &progress, &no_cancel(), usize::MAX, usize::MAX);
        results.sort();

        let mut expected: Vec<PathBuf> = (0..40).map(|i| dir.join(format!("dir_{i}")).join("target.txt")).collect();
        expected.sort();
        assert_eq!(results, expected);
    }

    #[test]
    fn matched_names_prunes_vcs_directories() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git").join("target.txt"), b"hi").unwrap();
        fs::write(dir.join("target.txt"), b"hi").unwrap();

        let progress = SearchProgress::default();
        let results = matched_names(&dir, "target", &progress, &no_cancel(), usize::MAX, usize::MAX);

        assert_eq!(results, vec![dir.join("target.txt")]);
    }

    /// The walk root itself is never checked against the query -- named
    /// so the scratch directory's own name (which contains "find-file")
    /// would match a query for it, if this ever regressed.
    #[test]
    fn matched_names_never_matches_the_walk_root_itself() {
        let dir = scratch_dir();
        fs::write(dir.join("other.txt"), b"hi").unwrap();

        let progress = SearchProgress::default();
        let results = matched_names(&dir, "find-file", &progress, &no_cancel(), usize::MAX, usize::MAX);

        assert!(results.is_empty(), "the root directory's own name should never be treated as a match");
    }

    /// Regression coverage for the real report: a search that happened
    /// to hit exactly `max_results` used to look like a complete,
    /// successful search with no way to tell it wasn't -- `capped`
    /// should be set whenever the result cap is what actually stopped
    /// the walk.
    #[test]
    fn matched_names_sets_capped_when_the_result_cap_is_hit() {
        let dir = scratch_dir();
        for i in 0..10 {
            fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        let progress = SearchProgress::default();
        let results = matched_names(&dir, "file", &progress, &no_cancel(), 5, usize::MAX);

        assert_eq!(results.len(), 5);
        assert!(progress.capped.load(Ordering::Relaxed), "hitting max_results should mark the search as capped");
    }

    #[test]
    fn matched_names_sets_capped_when_the_visited_cap_is_hit() {
        let dir = scratch_dir();
        for i in 0..10 {
            fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        let progress = SearchProgress::default();
        matched_names(&dir, "file", &progress, &no_cancel(), usize::MAX, 3);

        assert!(progress.capped.load(Ordering::Relaxed), "hitting max_visited should also mark the search as capped");
    }

    #[test]
    fn matched_names_does_not_set_capped_when_nothing_was_actually_truncated() {
        let dir = scratch_dir();
        fs::write(dir.join("target.txt"), b"hi").unwrap();

        let progress = SearchProgress::default();
        matched_names(&dir, "target", &progress, &no_cancel(), usize::MAX, usize::MAX);

        assert!(!progress.capped.load(Ordering::Relaxed), "a search that genuinely finished should not claim to be capped");
    }

    #[test]
    fn matched_names_stops_once_cancelled() {
        let dir = scratch_dir();
        for i in 0..50 {
            fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(true); // already cancelled before the walk even starts
        let results = matched_names(&dir, "file", &progress, &cancel, usize::MAX, usize::MAX);

        assert!(results.is_empty(), "a walk cancelled before it starts should find nothing");
    }

    #[test]
    fn matched_files_only_returns_files_not_directories() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("target_dir")).unwrap();
        fs::write(dir.join("target_file.txt"), b"hi").unwrap();

        let progress = SearchProgress::default();
        let results = matched_files(&dir, "target", &progress, &no_cancel(), usize::MAX);

        assert_eq!(results, vec![dir.join("target_file.txt")]);
    }
}
