use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize};

mod content;
mod matching;
mod walk;

/// Live counters a running search updates from any worker thread, read
/// by the UI for its "Searching... N visited, M found" line. `visited`
/// counts walked entries, then content-checked candidates (one counter
/// for both phases). `capped` records that a result/visit cap stopped
/// the search -- the count then shows "+" -- never set by `Esc`.
#[derive(Default)]
pub struct SearchProgress {
    pub visited: AtomicUsize,
    pub found: AtomicUsize,
    pub capped: AtomicBool,
}

/// Find file's search: a parallel walk (`walk.rs`, `ignore`'s walker
/// with its own filters off -- every real entry minus VCS metadata
/// directories), name matching (`matching.rs`), and, with "Text to
/// find", a parallel content check (`content/`). Bounded by
/// `limits().find_file_max_results`/`find_file_max_visited`; cancelled
/// through `cancel`. History: docs/history/find-file-search.md.
pub fn search_cancelable(root: &Path, query: &str, content_query: &str, progress: &SearchProgress, cancel: &AtomicBool) -> Vec<PathBuf> {
    let limits = crate::theming::config::limits();
    let query_lower = query.to_lowercase();
    let content_query_lower = content_query.to_lowercase();

    let mut results = if content_query_lower.is_empty() {
        walk::matched_names(root, &query_lower, progress, cancel, limits.find_file_max_results, limits.find_file_max_visited)
    } else {
        let candidates = walk::matched_files(root, &query_lower, progress, cancel, limits.find_file_max_visited);
        content::content_filter_in_parallel(candidates, &content_query_lower, limits.find_file_max_results, progress, cancel)
    };

    // Threads finish in any order. Sorting the full path groups results
    // by directory, sorted within, like Far's view; bounded by the cap.
    results.sort_by_cached_key(|path| path.to_string_lossy().to_lowercase());
    results
}

/// A thin wrapper around `search_cancelable` with a throwaway
/// progress/cancel pair neither this caller nor anything else ever
/// reads or sets -- test-only, since production code always has a real
/// `SearchProgress`/`AtomicBool` to hand it (`background.rs::spawn_search`
/// is the real, UI-driven entry point) and calling this from anywhere
/// else would just be dead weight.
#[cfg(test)]
fn search(root: &Path, query: &str, content_query: &str) -> Vec<PathBuf> {
    let progress = SearchProgress::default();
    let cancel = AtomicBool::new(false);
    search_cancelable(root, query, content_query, &progress, &cancel)
}


#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("find-file-search")
    }

    #[test]
    fn search_finds_a_matching_file_at_the_root() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();
        fs::write(dir.join("other.txt"), b"hi").unwrap();

        let results = search(&dir, "read", "");

        assert_eq!(results, vec![dir.join("readme.txt")]);
    }

    #[test]
    fn search_matches_case_insensitively() {
        let dir = scratch_dir();
        fs::write(dir.join("README.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "read", ""), vec![dir.join("README.txt")]);
    }

    #[test]
    fn search_descends_into_subdirectories() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested").join("target.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "target", ""), vec![dir.join("nested").join("target.txt")]);
    }

    #[test]
    fn search_matches_directory_names_too() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("target_dir")).unwrap();

        assert_eq!(search(&dir, "target", ""), vec![dir.join("target_dir")]);
    }

    /// Regression coverage for the real request: since both walk paths
    /// run in parallel, results arrive in whatever order worker threads
    /// happened to finish in -- reported directly, compared side by side
    /// against real Far Manager's own results view, which groups matches
    /// by directory with both directories and files sorted within it.
    /// `search_cancelable`'s own trailing sort should reproduce that same
    /// clustering-by-directory effect from a plain case-insensitive full-
    /// path sort, with no separate "group by directory" pass needed.
    #[test]
    fn search_results_are_sorted_case_insensitively_grouping_by_directory() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("Beta")).unwrap();
        fs::create_dir_all(dir.join("alpha")).unwrap();
        fs::write(dir.join("Beta").join("z_file.txt"), b"hi").unwrap();
        fs::write(dir.join("Beta").join("a_file.txt"), b"hi").unwrap();
        fs::write(dir.join("alpha").join("m_file.txt"), b"hi").unwrap();

        let results = search(&dir, "file", "");

        assert_eq!(
            results,
            vec![dir.join("alpha").join("m_file.txt"), dir.join("Beta").join("a_file.txt"), dir.join("Beta").join("z_file.txt")],
            "should sort case-insensitively (\"alpha\" before \"Beta\") and group each directory's own files together, sorted within it"
        );
    }

    /// Regression test for the actual reported bug: a file deep inside
    /// a large Subversion working copy wasn't found at all, because the
    /// walk exhausted `MAX_VISITED` inside `.svn`'s own pristine-copy
    /// store before ever reaching the real target directory. Here the
    /// noise dir only needs one bogus entry to prove it's skipped
    /// entirely, not actually large enough to hit the real cap.
    #[test]
    fn search_skips_vcs_metadata_directories() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join(".svn")).unwrap();
        fs::write(dir.join(".svn").join("target.txt"), b"hi").unwrap();
        fs::write(dir.join("target.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "target", ""), vec![dir.join("target.txt")]);
    }

    #[test]
    fn search_with_no_matches_returns_empty() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        assert!(search(&dir, "nope", "").is_empty());
    }

    /// Regression test for the actual reported bug: `*.md` used to be
    /// searched for as the literal six-character substring `"*.md"`,
    /// which matches nothing real, instead of as a glob pattern.
    #[test]
    fn search_treats_a_star_pattern_as_a_glob_not_a_literal_substring() {
        let dir = scratch_dir();
        fs::write(dir.join("README.md"), b"hi").unwrap();
        fs::write(dir.join("notes.txt"), b"hi").unwrap();

        let results = search(&dir, "*.md", "");

        assert_eq!(results, vec![dir.join("README.md")]);
    }

    #[test]
    fn search_glob_question_mark_matches_exactly_one_character() {
        let dir = scratch_dir();
        fs::write(dir.join("cat.txt"), b"hi").unwrap();
        fs::write(dir.join("cats.txt"), b"hi").unwrap();

        let results = search(&dir, "ca?.txt", "");

        assert_eq!(results, vec![dir.join("cat.txt")], "should match exactly one character, not \"cats\"'s two");
    }

    #[test]
    fn search_glob_star_can_match_the_empty_string() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "readme*.txt", ""), vec![dir.join("readme.txt")]);
    }

    #[test]
    fn search_content_query_filters_by_file_content() {
        let dir = scratch_dir();
        fs::write(dir.join("a.txt"), b"the quick brown fox").unwrap();
        fs::write(dir.join("b.txt"), b"the lazy dog").unwrap();

        let results = search(&dir, "", "brown");

        assert_eq!(results, vec![dir.join("a.txt")]);
    }

    #[test]
    fn search_content_query_is_case_insensitive() {
        let dir = scratch_dir();
        fs::write(dir.join("a.txt"), b"Hello World").unwrap();

        assert_eq!(search(&dir, "", "hello world"), vec![dir.join("a.txt")]);
    }

    #[test]
    fn search_content_query_combines_with_name_query() {
        let dir = scratch_dir();
        fs::write(dir.join("match.txt"), b"needle here").unwrap();
        fs::write(dir.join("match.md"), b"needle here too").unwrap();
        fs::write(dir.join("other.txt"), b"needle here as well").unwrap();

        let mut results = search(&dir, "match", "needle");
        results.sort();
        let mut expected = vec![dir.join("match.md"), dir.join("match.txt")];
        expected.sort();

        assert_eq!(results, expected, "both name-matched files also contain the needle");
    }

    #[test]
    fn search_content_query_excludes_directories_even_when_the_name_matches() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("target_dir")).unwrap();
        fs::write(dir.join("target_file.txt"), b"needle").unwrap();

        let results = search(&dir, "target", "needle");

        assert_eq!(results, vec![dir.join("target_file.txt")], "a directory has no content to search inside, so it can't satisfy a content query");
    }

    #[test]
    fn search_content_query_with_no_content_match_returns_empty() {
        let dir = scratch_dir();
        fs::write(dir.join("a.txt"), b"nothing relevant here").unwrap();

        assert!(search(&dir, "", "needle").is_empty());
    }

    #[test]
    fn search_content_query_skips_files_it_cannot_read_as_utf8() {
        let dir = scratch_dir();
        fs::write(dir.join("binary.dat"), [0xFF, 0xFE, 0x00, 0x01, 0xC0]).unwrap();

        assert!(search(&dir, "binary", "needle").is_empty(), "an unreadable file should be treated as not matching, not error out the whole search");
    }

    /// Regression coverage for the real request: the popup should be
    /// able to show live progress while a search runs, not just a
    /// final count once it's done.
    #[test]
    fn search_cancelable_reports_progress_as_it_goes() {
        let dir = scratch_dir();
        fs::write(dir.join("a.txt"), b"hi").unwrap();
        fs::write(dir.join("b.txt"), b"hi").unwrap();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(false);
        let results = search_cancelable(&dir, "read", "", &progress, &cancel);

        assert_eq!(results, vec![dir.join("readme.txt")]);
        assert_eq!(progress.visited.load(Ordering::Relaxed), 3, "should have visited every entry in the directory");
        assert_eq!(progress.found.load(Ordering::Relaxed), 1, "should track the one real match");
    }

    /// A search that's already been asked to cancel before it even
    /// starts should stop almost immediately, finding nothing past
    /// whatever it managed to visit before noticing -- `Esc` during
    /// `FindFilePhase::Searching` sets this flag from a separate thread
    /// (`background.rs::PendingSearch::cancel`), so the check has to be
    /// genuinely effective, not just present.
    #[test]
    fn search_cancelable_stops_once_cancelled() {
        let dir = scratch_dir();
        for i in 0..50 {
            fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        let progress = SearchProgress::default();
        let cancel = AtomicBool::new(true); // already cancelled before the search even starts
        let results = search_cancelable(&dir, "file", "", &progress, &cancel);

        assert!(results.is_empty(), "a search cancelled before it starts should find nothing");
    }
}
