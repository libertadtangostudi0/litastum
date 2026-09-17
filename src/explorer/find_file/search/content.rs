use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::SearchProgress;

/// Checks every candidate's own content for `needle_lower`
/// (`file_contains`) across a fixed pool of threads
/// (`available_parallelism()`, falling back to 1 if the OS won't say),
/// stopping early once `max_results` matches are found or `cancel` is
/// set. See `search/mod.rs`'s own doc comment for why this runs in
/// parallel at all. `progress.visited` keeps counting up across this
/// second phase (it doesn't reset to 0 -- see `SearchProgress`'s own
/// doc comment for why one running counter is enough here), now
/// meaning "candidates content-checked so far" rather than "directory
/// entries walked."
///
/// **Work distribution is a shared `next_index` counter, not a static
/// chunk per thread** -- an earlier version split `candidates` into
/// `threads` equal-sized slices up front, which starves under a real,
/// uneven candidate list: file sizes vary wildly (a handful of huge log
/// files mixed in with thousands of small source files), so a thread
/// unlucky enough to get a slice with the big ones sits busy long after
/// every other thread has run out of work and gone idle. Every thread
/// here instead pulls whichever index is next via one shared
/// `fetch_add` and keeps going until the counter runs past the end of
/// `candidates` -- self-balancing, since a thread that finishes a small
/// file quickly just claims another index sooner, without needing a
/// real work-stealing deque (`crossbeam-deque`, pulled in transitively
/// by the `ignore` crate `walk.rs` already depends on, would be the
/// next step up if this single shared counter ever stopped being
/// enough -- not needed here, since claiming *the next unclaimed
/// index* is already the whole of what stealing would buy on a flat
/// list like this one).
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
                if cancel.load(Ordering::Relaxed) || found.lock().unwrap().len() >= max_results {
                    return;
                }
                let index = next_index.fetch_add(1, Ordering::Relaxed);
                let Some(path) = candidates.get(index) else {
                    return; // no more candidates left to claim
                };

                let matched = file_contains(path, needle_lower);
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

/// Chunk size for `file_contains`'s own streaming read -- large enough
/// that most real text files (source, logs) finish in one or two
/// reads, small enough that a huge file's own match, once found, is
/// found without reading much further past it. Not tuned against real
/// benchmarks, just a reasonable middle ground -- revisit if a real
/// case shows it matters.
const CONTENT_CHUNK_SIZE: usize = 64 * 1024;

/// Whether `path`'s own content contains `needle_lower` (already
/// lowercased by the caller) as a substring, case-insensitively. Skips
/// straight to "no match" for a file `looks_binary` flags first --
/// see its own doc comment for why that's worth checking before paying
/// for the real streamed scan below.
fn file_contains(path: &Path, needle_lower: &str) -> bool {
    if looks_binary(path) {
        return false;
    }
    file_contains_with_chunk_size(path, needle_lower, CONTENT_CHUNK_SIZE)
}

/// How many leading bytes to sniff for a null byte when deciding
/// whether a file is probably binary -- 8000 is the same convention
/// git and ripgrep both use for this exact check: large enough that a
/// real text file's own unusual leading bytes (a BOM, some odd
/// whitespace) won't misjudge it, small enough that sniffing costs
/// nothing next to the full scan it exists to skip.
const BINARY_SNIFF_LEN: usize = 8000;

/// A cheap, approximate "don't bother fully scanning this" check --
/// reads at most `BINARY_SNIFF_LEN` bytes from the start of `path` and
/// looks for a null byte, same heuristic git/ripgrep use to skip
/// binary files without a real parse. Added directly after a
/// side-by-side comparison against Far Manager's own content search:
/// a tree with a real mix of source files and binaries (`.exe`, `.dll`,
/// images, ...) used to stream every single one of those binaries
/// through `file_contains_with_chunk_size` in full, just to hit an
/// invalid-UTF-8 byte and bail -- often not until well past the first
/// chunk, since compiled binaries frequently have long valid-looking
/// ASCII runs (string tables, padding) before the byte that actually
/// breaks decoding. Reading a real file's whole content just to
/// discover partway through that it was never text to begin with is
/// exactly the wasted work this sniff avoids.
///
/// Not perfect -- a null byte can legitimately appear in some encodings
/// this app doesn't otherwise search anyway (UTF-16, for instance,
/// which `file_contains_with_chunk_size` would also reject as invalid
/// UTF-8 immediately regardless) -- but cheap, and right in the
/// overwhelmingly common case this app's own file panel already sees:
/// real binaries next to real UTF-8 text, not exotic encodings.
/// Opens `path` a second time rather than sharing a handle with the
/// real scan that follows a `false` result -- two cheap opens plus one
/// small read is still far below the cost of a full scan for any real
/// file, and keeps this check fully independent of
/// `file_contains_with_chunk_size`'s own streaming state instead of
/// threading a "already sniffed this many bytes" starting point through
/// it.
fn looks_binary(path: &Path) -> bool {
    use std::io::Read;

    let Ok(mut file) = fs::File::open(path) else {
        return true; // unreadable either way -- nothing further to scan
    };
    let mut buf = [0u8; BINARY_SNIFF_LEN];
    let Ok(read) = file.read(&mut buf) else {
        return true;
    };
    buf[..read].contains(&0)
}

/// Streams `path` in fixed-size byte chunks (`std::io::Read`, not
/// `fs::read_to_string`) and stops at the file's own first match,
/// rather than always reading a candidate to the end -- same shape
/// real Far Manager's own "Text to find" uses (see `search/mod.rs`'s
/// own doc comment for the fuller comparison), and a real win for a
/// large file whose match sits early: the old whole-file
/// `fs::read_to_string` + `.to_lowercase()` read every byte and
/// allocated two full copies of it (the read buffer and the lowercased
/// string) no matter where -- or whether -- a match actually was. A
/// file that isn't valid UTF-8 text (binary, unreadable, ...) is still
/// treated as not matching rather than erroring the whole search out
/// over one candidate. `chunk_size` is a parameter only so tests can
/// force cross-chunk-boundary matches deterministically with a tiny
/// value; `file_contains` itself always calls this with
/// `CONTENT_CHUNK_SIZE`.
///
/// Correctness across chunk boundaries -- both a multi-byte UTF-8
/// character and the needle itself can straddle two reads:
/// `pending_bytes` carries over any trailing byte sequence that didn't
/// yet decode as a complete character, so it's prepended to (not lost
/// before) the next read; `carry` keeps the lowercased tail of
/// already-decoded text, trimmed back down to roughly the needle's own
/// length after each check, so a match starting a few bytes before a
/// chunk boundary is still seen once the next chunk arrives instead of
/// being split across two independent, non-overlapping searches.
fn file_contains_with_chunk_size(path: &Path, needle_lower: &str, chunk_size: usize) -> bool {
    use std::io::Read;

    if needle_lower.is_empty() {
        return true;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file);
    let mut raw = vec![0u8; chunk_size];
    let mut pending_bytes: Vec<u8> = Vec::new();
    let mut carry = String::new();
    let needle_chars = needle_lower.chars().count();

    loop {
        let read = match reader.read(&mut raw) {
            Ok(0) => return false, // EOF, no match found in any chunk
            Ok(n) => n,
            Err(_) => return false,
        };
        pending_bytes.extend_from_slice(&raw[..read]);

        let (decoded, incomplete_tail_len) = match std::str::from_utf8(&pending_bytes) {
            Ok(text) => (text.to_string(), 0),
            // `error_len().is_none()` means the bytes just *end* mid-
            // character (routine with an arbitrary chunk boundary, not
            // evidence the file is actually binary) -- decode the
            // valid prefix now and carry the incomplete tail into the
            // next read, where more bytes might complete it.
            Err(err) if err.error_len().is_none() => {
                let valid_len = err.valid_up_to();
                let text = std::str::from_utf8(&pending_bytes[..valid_len]).unwrap().to_string();
                (text, pending_bytes.len() - valid_len)
            }
            Err(_) => return false, // a real invalid byte sequence, not just a split character
        };
        pending_bytes.drain(..pending_bytes.len() - incomplete_tail_len);

        carry.push_str(&decoded.to_lowercase());
        if carry.contains(needle_lower) {
            return true;
        }
        let keep_from = carry.char_indices().rev().nth(needle_chars.saturating_sub(1)).map(|(i, _)| i).unwrap_or(0);
        if keep_from > 0 {
            carry.drain(..keep_from);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("find-file-search-content")
    }

    /// Regression coverage for `file_contains_with_chunk_size`'s own
    /// cross-chunk-boundary handling -- a tiny chunk size forces a real
    /// multi-read scan even over a short string, deterministically
    /// exercising the `carry`/`pending_bytes` logic that a real
    /// `CONTENT_CHUNK_SIZE` (64 KiB) read almost never would in a test.
    #[test]
    fn file_contains_finds_a_match_that_straddles_a_chunk_boundary() {
        let dir = scratch_dir();
        let path = dir.join("straddle.txt");
        // "needle" placed so a 4-byte chunk boundary falls in the
        // middle of it (bytes 0..4 = "xxxn", 4..8 = "eedl", ...).
        fs::write(&path, b"xxxneedlexxx").unwrap();

        assert!(file_contains_with_chunk_size(&path, "needle", 4));
    }

    #[test]
    fn file_contains_with_a_tiny_chunk_size_finds_no_match_when_there_is_none() {
        let dir = scratch_dir();
        let path = dir.join("no_match.txt");
        fs::write(&path, b"xxxxxxxxxxxxxxxxxxxx").unwrap();

        assert!(!file_contains_with_chunk_size(&path, "needle", 4));
    }

    /// A multi-byte UTF-8 character split across a chunk boundary
    /// should still decode correctly, not be treated as invalid UTF-8
    /// and silently fail the whole file -- `pending_bytes` exists
    /// specifically to carry an incomplete character's own leading
    /// bytes into the next read instead.
    #[test]
    fn file_contains_handles_a_multi_byte_character_split_across_a_chunk_boundary() {
        let dir = scratch_dir();
        let path = dir.join("multibyte.txt");
        // "grüßen" -- "ü" and "ß" are each two UTF-8 bytes; a 3-byte
        // chunk size guarantees at least one of them gets split.
        fs::write(&path, "grüßen".as_bytes()).unwrap();

        assert!(file_contains_with_chunk_size(&path, "üßen", 3));
        assert!(!file_contains_with_chunk_size(&path, "notpresent", 3));
    }

    #[test]
    fn file_contains_is_case_insensitive_across_chunk_boundaries() {
        let dir = scratch_dir();
        let path = dir.join("case.txt");
        fs::write(&path, b"xxxNEEDLExxx").unwrap();

        assert!(file_contains_with_chunk_size(&path, "needle", 4));
    }

    #[test]
    fn looks_binary_is_true_for_content_with_a_null_byte() {
        let dir = scratch_dir();
        let path = dir.join("binary.dat");
        fs::write(&path, [0x41, 0x42, 0x00, 0x43]).unwrap();

        assert!(looks_binary(&path));
    }

    #[test]
    fn looks_binary_is_false_for_plain_text() {
        let dir = scratch_dir();
        let path = dir.join("plain.txt");
        fs::write(&path, b"just some ordinary text, no null bytes here").unwrap();

        assert!(!looks_binary(&path));
    }

    /// `file_contains` should skip a binary file entirely, via
    /// `looks_binary`, rather than needing `file_contains_with_chunk_size`
    /// to run into invalid UTF-8 partway through a real streamed scan
    /// to reach the same "not a match" conclusion.
    #[test]
    fn file_contains_treats_a_binary_file_as_not_matching_without_a_full_scan() {
        let dir = scratch_dir();
        let path = dir.join("binary.dat");
        // The needle itself is present as real bytes, but preceded by a
        // null byte -- `looks_binary` should reject this before the
        // real scan ever gets a chance to find it.
        let mut content = vec![0x00];
        content.extend_from_slice(b"needle");
        fs::write(&path, content).unwrap();

        assert!(!file_contains(&path, "needle"));
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

    /// Regression coverage for the shared-`next_index` rewrite (replacing
    /// the earlier static equal-chunks-per-thread split): every matching
    /// candidate should still be found exactly once, across a candidate
    /// list large and uneven enough (most files tiny, a few deliberately
    /// large) to actually exercise more than one thread claiming more
    /// than its own "fair share" of the work.
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
