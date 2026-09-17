use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};

use crate::explorer::is_vcs_dir_name;

use super::matching::matches_query;
use super::SearchProgress;

/// A `WalkBuilder` configured the way this app's own search has always
/// behaved: every real entry under `root`, minus known VCS metadata
/// directories (`.git`/`.svn`/`.hg`/`.bzr`, `is_vcs_dir_name`) -- not
/// `ignore`'s own default behavior, which is built around `.gitignore`/
/// hidden-file/`.ignore` filtering (exactly what ripgrep wants, not
/// what a plain "find a file by name" dialog should silently apply).
/// `standard_filters(false)` turns every one of those off; `filter_entry`
/// then adds back only the one exclusion this app has ever actually
/// made — pruning VCS directories, not gitignore-aware filtering. A
/// `.gitignore`'d file is still a real file on disk and should still be
/// findable here, same as it always was.
fn build_walker(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.standard_filters(false);
    builder.filter_entry(|entry| !(entry.file_type().is_some_and(|file_type| file_type.is_dir()) && is_vcs_dir_name(&entry.file_name().to_string_lossy())));
    builder
}

/// Recursively walks `root` in parallel (`ignore::WalkBuilder::build_parallel`
/// — the same crate, and the same walking primitive, ripgrep itself
/// uses for this exact job), returning every entry (file *or*
/// directory) whose name matches `query_lower`, up to `max_results`,
/// visiting at most `max_visited` entries in total. See `search/mod.rs`'s
/// own doc comment for why a hand-rolled work-stealing walker wasn't
/// attempted here instead — directory subtrees vary wildly in size, so
/// a flat, evenly-sized chunking (the shape `content_filter_in_parallel`
/// already uses for its own flat candidate list) doesn't fit a tree
/// walk the way it fits a list.
///
/// The root path itself (`ignore`'s own depth-0 entry) is never checked
/// against `query_lower` -- matches the original single-threaded walk's
/// own behavior, which only ever iterated a directory's *children* via
/// `fs::read_dir`, never asked whether the directory passed in was
/// itself a match.
///
/// Multiple worker threads race to increment `progress.visited`/push
/// into the shared `results` `Mutex` -- `WalkState::Quit` (returned once
/// `cancel` is set, or either cap is reached) stops the walk "as soon as
/// possible," per `ignore`'s own documented semantics, not instantly;
/// a handful of entries past a cap can still be visited by other
/// threads already mid-directory when the signal lands. Acceptable —
/// the caps were never meant to be exact boundaries, just backstops
/// against a huge tree, and this is the same trade-off already made for
/// `cancel` in `content_filter_in_parallel`.
pub(super) fn matched_names(root: &Path, query_lower: &str, progress: &SearchProgress, cancel: &AtomicBool, max_results: usize, max_visited: usize) -> Vec<PathBuf> {
    let results: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

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

            let name = entry.file_name().to_string_lossy().to_lowercase();
            if matches_query(&name, query_lower) {
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
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if matches_query(&name, query_lower) {
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
