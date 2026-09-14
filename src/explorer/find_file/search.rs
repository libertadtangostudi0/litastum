use std::fs;
use std::path::{Path, PathBuf};

use crate::explorer::is_vcs_dir_name;

/// **Known potential performance weak spot — revisit if this is ever
/// actually slow, not preemptively.** `search`/`search_into` below is a
/// plain, synchronous, single-threaded recursive `fs::read_dir` walk,
/// run directly on the key-handling thread: it blocks the whole UI
/// (no spinner, no cancel) until it finishes, and only VCS metadata
/// directories (`.git`/`.svn`/`.hg`/`.bzr`, see `is_vcs_dir_name` below)
/// are pruned by default — other common noise (`target`, `node_modules`,
/// ...) is not — the only other safety net against a huge tree is the
/// `MAX_RESULTS`/`MAX_VISITED` caps below, which bound the damage but
/// don't make a big search *fast*. Fine for the repo sizes this has
/// actually been tried against; a genuinely large tree (a monorepo, a
/// deep `node_modules`) could still make this noticeably slow or
/// briefly freeze the UI. If that ever becomes a real complaint rather
/// than a theoretical one, the fix directions are, in roughly
/// increasing effort: skip more well-known noise directories by
/// default (`ignore`-crate-style pruning), run the walk on a
/// background thread with a cancel key and a progress indicator, or
/// parallelize the walk itself (e.g. `rayon`/`ignore::WalkBuilder`).
/// None of that is done here — this file is deliberately the simplest
/// thing that could work, with the risk written down instead of
/// silently discovered later at a bad moment.
///
/// **VCS directory pruning was added after a real report**: searching
/// a Subversion working copy for a file several directories deep
/// (`rxclass_imp.cpp` under `IntelliCAD/Source/IntelliCAD/lib/IcArx/`)
/// returned "No matches found", while real Far Manager found it
/// instantly. Root cause was `MAX_VISITED` itself, not a matching bug:
/// SVN's `.svn` metadata directory keeps a full pristine copy of every
/// versioned file (`.svn/pristine/`), so a working copy's own `.svn`
/// subtree alone can easily contain tens of thousands of entries —
/// comfortably enough to exhaust the old `MAX_VISITED` (50,000, at the
/// time) before the walk (plain `fs::read_dir` order, not sorted, not
/// prioritized) ever reached the real target directory. `.git` has the
/// same shape of problem (a full packed object store) even though it
/// usually stays more compact; `.hg`/`.bzr` are pruned too for the same
/// reason, for consistency with the VCS-directory list already used
/// for file-panel coloring (`explorer/entry.rs::is_vcs_dir_name`)
/// rather than hand-picking a different list here.
///
/// Results are capped, and the walk itself gives up after visiting
/// this many entries — a huge tree shouldn't be able to hang the UI
/// indefinitely even without directory exclusions. `MAX_VISITED` itself
/// was raised from its original 50,000 once VCS-directory pruning above
/// removed the main reason a real tree could blow through it — a plain
/// synchronous `fs::read_dir` walk over a few million entries still
/// only takes on the order of a second on a local disk, so this is
/// still a "shouldn't hang forever" backstop, not a tuned performance
/// budget; lower it back down if a genuinely huge non-VCS tree (a deep
/// `node_modules`, a build output dir) is ever reported as freezing the
/// UI for real.
const MAX_RESULTS: usize = 200;
const MAX_VISITED: usize = 2_000_000;

/// Recursively searches `root` for entries whose file name matches
/// `query` (case-insensitively), returning matching paths in the order
/// found (a plain `fs::read_dir` walk order, not sorted — good enough
/// for a first pass at this feature). See `MAX_RESULTS`/`MAX_VISITED`
/// above for the safety caps, and this module's own doc comment for
/// the performance caveat.
///
/// `query` is a glob pattern (`*`/`?`, Far Manager's own convention for
/// this dialog — `*.md`, `read?e.txt`) if it contains either wildcard
/// character; otherwise it's a plain substring, which covers the
/// common "just type part of the name" case without forcing `*name*`
/// on every query. Found missing by hand: `*.md` was searched for
/// *literally* (as the six-character substring `"*.md"`, which no real
/// file name contains) before this distinction existed.
pub fn search(root: &Path, query: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut visited = 0;
    let query_lower = query.to_lowercase();
    search_into(root, &query_lower, &mut results, &mut visited);
    results
}

fn search_into(dir: &Path, query_lower: &str, results: &mut Vec<PathBuf>, visited: &mut usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(|entry| entry.ok()) {
        if results.len() >= MAX_RESULTS || *visited >= MAX_VISITED {
            return;
        }
        *visited += 1;

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if matches_query(&name, query_lower) {
            results.push(path.clone());
        }

        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) && !is_vcs_dir_name(&entry.file_name().to_string_lossy()) {
            search_into(&path, query_lower, results, visited);
        }
    }
}

/// `name` and `query_lower` are both assumed already lowercased by the
/// caller (`search`/`search_into`). Glob semantics only kick in once
/// `query_lower` actually contains a wildcard character.
fn matches_query(name: &str, query_lower: &str) -> bool {
    if query_lower.contains('*') || query_lower.contains('?') {
        glob_match(query_lower, name)
    } else {
        name.contains(query_lower)
    }
}

/// Classic greedy `*`/`?` wildcard matching (`*` — any run of
/// characters, including none; `?` — exactly one character) — the
/// textbook two-pointer-plus-backtrack-point algorithm, not a
/// full-featured glob (no `[...]` character classes, no escaping).
/// Operates on `char`s rather than bytes so a multi-byte file name
/// can't be split mid-character.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    // Where the most recent unresolved `*` sits in `pattern`, and how
    // far into `text` we've tried stretching it to cover so far --
    // `None` until the first `*` is seen, since there's nothing to
    // backtrack to before that.
    let mut star_p: Option<usize> = None;
    let mut star_t = 0;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star_p = Some(p);
            star_t = t;
            p += 1;
        } else if let Some(sp) = star_p {
            // The match after the last `*` failed -- stretch that `*`
            // to cover one more character and retry from right after it.
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
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

        let results = search(&dir, "read");

        assert_eq!(results, vec![dir.join("readme.txt")]);
    }

    #[test]
    fn search_matches_case_insensitively() {
        let dir = scratch_dir();
        fs::write(dir.join("README.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "read"), vec![dir.join("README.txt")]);
    }

    #[test]
    fn search_descends_into_subdirectories() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested").join("target.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "target"), vec![dir.join("nested").join("target.txt")]);
    }

    #[test]
    fn search_matches_directory_names_too() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("target_dir")).unwrap();

        assert_eq!(search(&dir, "target"), vec![dir.join("target_dir")]);
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

        assert_eq!(search(&dir, "target"), vec![dir.join("target.txt")]);
    }

    #[test]
    fn search_with_no_matches_returns_empty() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        assert!(search(&dir, "nope").is_empty());
    }

    /// Regression test for the actual reported bug: `*.md` used to be
    /// searched for as the literal six-character substring `"*.md"`,
    /// which matches nothing real, instead of as a glob pattern.
    #[test]
    fn search_treats_a_star_pattern_as_a_glob_not_a_literal_substring() {
        let dir = scratch_dir();
        fs::write(dir.join("README.md"), b"hi").unwrap();
        fs::write(dir.join("notes.txt"), b"hi").unwrap();

        let results = search(&dir, "*.md");

        assert_eq!(results, vec![dir.join("README.md")]);
    }

    #[test]
    fn search_glob_question_mark_matches_exactly_one_character() {
        let dir = scratch_dir();
        fs::write(dir.join("cat.txt"), b"hi").unwrap();
        fs::write(dir.join("cats.txt"), b"hi").unwrap();

        let results = search(&dir, "ca?.txt");

        assert_eq!(results, vec![dir.join("cat.txt")], "should match exactly one character, not \"cats\"'s two");
    }

    #[test]
    fn search_glob_star_can_match_the_empty_string() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "readme*.txt"), vec![dir.join("readme.txt")]);
    }

    #[test]
    fn glob_match_examples() {
        assert!(glob_match("*.md", "readme.md"));
        assert!(!glob_match("*.md", "readme.txt"));
        assert!(glob_match("read?e.txt", "readme.txt"));
        assert!(!glob_match("read?e.txt", "readmme.txt"), "? is exactly one character, not one-or-more");
        assert!(glob_match("*", "anything.at.all"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"), "missing the trailing c");
        assert!(glob_match("", ""));
        assert!(!glob_match("a", ""));
        assert!(glob_match("*", ""), "a bare * matches even an empty string");
    }
}
