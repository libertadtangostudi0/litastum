use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize};

mod content;
mod matching;
mod walk;

/// Live counters a running search updates as it goes -- `visited` is
/// entries walked (directory phase) or candidates content-checked
/// (content-filter phase, sharing the same counter rather than a
/// second one; "how far along the current phase is" reads fine either
/// way for a plain progress line, and a real two-phase breakdown
/// wasn't asked for), `found` is how many have actually become results
/// so far. Both are updated from whichever thread happens to process a
/// given entry or candidate -- the directory walk (`walk.rs`) and the
/// content check (`content.rs`) are both parallel now, not just the
/// content check. Read from the UI thread
/// (`ui/find_file.rs::draw_searching`) while a background search
/// (`background.rs::spawn_search`) is still running, so a
/// "Searching... N visited, M found" line can update live instead of
/// the popup just sitting blank until the whole search finishes.
///
/// `capped` -- set once, by whichever phase actually hits
/// `find_file_max_results`/`find_file_max_visited` first
/// (`walk.rs`/`content.rs`, wherever the real early-exit happens) --
/// records that the search stopped *because of a cap*, not because it
/// genuinely ran out of real matches. Added after a direct report: a
/// search that hit exactly `find_file_max_results` (200, the default)
/// silently looked like a complete, successful search with no way to
/// tell it wasn't -- compared side by side against real Far Manager on
/// the same tree, which found more than double that. `ui/find_file.rs`
/// reads this once the search finishes (`FindFileState::results_capped`)
/// to show a "+" on the result count instead of claiming a precise,
/// possibly-wrong total. Never set on a plain `cancel` (`Esc`) --
/// that's a deliberate, user-initiated stop, not a surprising
/// incompleteness the popup needs to call out separately.
#[derive(Default)]
pub struct SearchProgress {
    pub visited: AtomicUsize,
    pub found: AtomicUsize,
    pub capped: AtomicBool,
}

/// **This module (`find_file/search/`) is Find file's actual search
/// engine — see `find_file.rs`'s own doc comment for how it fits into
/// the popup as a whole.** Split by concern once it passed this
/// project's own ~500-line decomposition threshold (`code-conventions.md`):
/// `walk.rs` (recursively finding candidate paths), `matching.rs` (does
/// one name match a typed query/glob), `content.rs` (does a candidate
/// file's own content match a "Text to find" substring), tied together
/// here.
///
/// **History, roughly in the order real reports/requests landed, kept
/// here since each one explains a real design choice still visible in
/// this module today:**
/// 1. A synchronous, single-threaded, plain recursive `fs::read_dir`
///    walk, run directly on the key-handling thread, with only known
///    VCS metadata directories (`.git`/`.svn`/`.hg`/`.bzr`,
///    `is_vcs_dir_name`) pruned — added specifically after a report
///    that searching a real Subversion working copy for a file several
///    directories deep returned "No matches found": SVN's `.svn`
///    metadata directory keeps a full pristine copy of every versioned
///    file, easily large enough to exhaust the old visited-entry cap
///    before the walk ever reached the real target directory.
/// 2. Far Manager's own second "Text to find" field (a content
///    substring, AND-combined with the name/mask match) — a *directory*
///    is dropped entirely once this is set, since there's nothing
///    inside one to search.
/// 3. Reported almost immediately against a real tree with hundreds of
///    thousands of files and a broad `*.*` mask: reading every
///    name-matched file's own content one at a time, on the same
///    thread already blocking the UI, made a genuinely large search
///    indistinguishable from a hung one. Fixed by running the content
///    check across a thread pool (`content.rs::content_filter_in_parallel`)
///    — explicitly *not* by capping file size or count, since a large
///    real tree was the normal case being reported, not something to
///    search less of.
/// 4. Asked directly for a comparison against real Far Manager's own
///    dialog, which streams a candidate file in chunks and stops at its
///    own first match rather than reading it whole — `content.rs::file_contains_with_chunk_size`
///    replaced the old `fs::read_to_string` + `.to_lowercase()` whole-file
///    read for the same reason `content_filter_in_parallel` above
///    exists: real, reported cost, not a theoretical one.
/// 5. Asked directly for the search to show live progress and support
///    `Esc`-to-cancel, matching real Far's own dialog — `SearchProgress`
///    (this file) plus an `AtomicBool` threaded through every walk/
///    check function, and `find_file/background.rs`'s own background-
///    thread machinery (mirroring `explorer::image_preview`'s already-
///    established pattern) to actually run a search without blocking
///    the UI at all.
/// 6. **This rewrite**: asked directly, as an explicit next step once
///    the above closed the "no feedback, no cancel" gap but left the
///    walk itself exactly as single-threaded as before — `walk.rs` now
///    walks in parallel too, via `ignore::WalkBuilder::build_parallel`
///    (the same crate, and the same walking primitive, ripgrep itself
///    uses for this exact job). A hand-rolled work-stealing walker
///    (a shared directory queue drained by a fixed thread pool) was
///    considered and rejected: `content_filter_in_parallel`'s own flat,
///    evenly-sized chunking works precisely because its input is a flat
///    list known up front — a directory tree isn't; subtree sizes vary
///    wildly (a `.git` object store next to a single-file directory),
///    so a hand-rolled equivalent would need its own real work-stealing
///    logic to avoid one thread finishing in a millisecond while
///    another chews through a huge subtree alone, which `ignore`
///    already provides, tested, as the literal reason ripgrep is fast
///    on huge trees. Every one of `ignore`'s own default filters
///    (`.gitignore`, hidden files, `.ignore`, git excludes) is turned
///    off (`walk.rs::build_walker`) — this app's own scope has always
///    been "every real entry, minus VCS metadata directories," not
///    gitignore-aware filtering, and turning `ignore`'s own filtering on
///    would silently change what a search finds versus what it found
///    before this rewrite.
///
/// **Still open**: no default exclusion of other common noise
/// (`target/`, `node_modules/`, ...) beyond VCS metadata directories —
/// only `MAX_RESULTS`/`MAX_VISITED` (`theming::config::limits()`) bound
/// the damage on a tree full of it. `ignore`'s own gitignore-style
/// pruning could cover this for free if ever wanted, but turning it on
/// changes *what a search finds*, not just how fast it runs, so it's a
/// real design question (silently skip gitignored real files, or ask
/// for permission first), not free performance — left alone rather than
/// bundled into this rewrite, which was scoped to speed, not to
/// changing which files a search can find.
pub fn search_cancelable(root: &Path, query: &str, content_query: &str, progress: &SearchProgress, cancel: &AtomicBool) -> Vec<PathBuf> {
    let limits = crate::theming::config::limits();
    let query_lower = query.to_lowercase();
    let content_query_lower = content_query.to_lowercase();

    if content_query_lower.is_empty() {
        return walk::matched_names(root, &query_lower, progress, cancel, limits.find_file_max_results, limits.find_file_max_visited);
    }

    let candidates = walk::matched_files(root, &query_lower, progress, cancel, limits.find_file_max_visited);
    content::content_filter_in_parallel(candidates, &content_query_lower, limits.find_file_max_results, progress, cancel)
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
