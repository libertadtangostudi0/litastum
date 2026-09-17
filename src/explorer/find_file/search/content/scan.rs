use std::fs;
use std::io::Read;
use std::path::Path;

/// Chunk size for `file_contains`'s own streaming read -- large enough
/// that most real text files (source, logs) finish in one or two
/// reads, small enough that a huge file's own match, once found, is
/// found without reading much further past it. Also doubles as the
/// binary-sniff window (`looks_binary`) -- see `file_contains_with_chunk_size`'s
/// own doc comment for why one read serves both purposes now. Not
/// tuned against real benchmarks, just a reasonable middle ground --
/// revisit if a real case shows it matters.
const CONTENT_CHUNK_SIZE: usize = 64 * 1024;

/// Whether `path`'s own content contains `needle_lower` (already
/// lowercased by the caller) as a substring, case-insensitively.
pub(super) fn file_contains(path: &Path, needle_lower: &str) -> bool {
    file_contains_with_chunk_size(path, needle_lower, CONTENT_CHUNK_SIZE)
}

/// Streams `path` in fixed-size byte chunks (`std::io::Read`, not
/// `fs::read_to_string`) and stops at the file's own first match,
/// rather than always reading a candidate to the end -- same shape
/// real Far Manager's own "Text to find" uses (see `search/mod.rs`'s
/// own doc comment for the fuller comparison), and a real win for a
/// large file whose match sits early: the old whole-file
/// `fs::read_to_string` + `.to_lowercase()` read every byte and
/// allocated two full copies of it (the read buffer and the lowercased
/// string) no matter where -- or whether -- a match actually was.
/// `chunk_size` is a parameter only so tests can force cross-chunk-
/// boundary matches deterministically with a tiny value; `file_contains`
/// itself always calls this with `CONTENT_CHUNK_SIZE`.
///
/// **Opens the file exactly once**, reading its own first chunk before
/// deciding anything -- an earlier version opened `path` twice: once
/// for a separate, smaller "does this look binary" sniff
/// (`looks_binary`, then a standalone read of its own), then again for
/// the real scan if that came back clean. Two file opens plus two reads
/// is real, avoidable overhead multiplied by every name-matched
/// candidate in a content-query search -- merged into one open + one
/// first read here: `looks_binary` now just inspects whatever bytes
/// this call already has in hand (the *whole* first `chunk_size`
/// chunk, comfortably larger than the old dedicated 8000-byte sniff
/// window, so accuracy only improved), and that same chunk becomes the
/// real scan's own first iteration instead of being read a second time.
///
/// **Dispatches on whether `needle_lower` is itself ASCII** -- reported
/// directly, compared file-by-file against real Far Manager on the same
/// tree (litastum: 1778 results, Far: 1783 -- a handful of real files
/// silently missing, not a cap: `find_file_max_results` wasn't hit).
/// Root cause, confirmed by hand against one of the missing files: a
/// `#pragma managed(push, off)` line sitting in plain ASCII, in a file
/// that also has genuinely non-UTF-8 bytes elsewhere (a legacy source
/// file with `Windows-1251`-encoded Cyrillic comments, common in an
/// older, internationally-authored C++ codebase, saved before the
/// project settled on UTF-8 throughout). The old, sole implementation
/// here (now `scan_utf8_text`) decodes each chunk as UTF-8 and gives up
/// the instant it hits a genuinely invalid byte sequence -- correct for
/// a needle that itself needs real Unicode case-folding, but far too
/// strict for the overwhelmingly common case of a plain ASCII needle
/// (`"pragma"`, a function name, `TODO`, ...): an ASCII byte sequence
/// reads identically whether the *rest* of the file is UTF-8,
/// Windows-125x, ISO-8859-x, or any other encoding that's
/// ASCII-compatible in the 0–127 range, which covers virtually every
/// real-world 8-bit encoding actually used for source code (UTF-16 is
/// the real exception -- its interleaved null bytes break a contiguous
/// ASCII match regardless of how it's searched, and isn't what this fix
/// targets). `scan_ascii_bytes` below searches raw bytes directly for
/// exactly this case, with no UTF-8 validity requirement on the file at
/// all -- real Far Manager's own search is evidently doing something
/// equivalent, which is why it kept finding matches litastum's old
/// UTF-8-only scan gave up on partway through the file. A non-ASCII
/// needle (searching for literal non-ASCII text) still goes through
/// `scan_utf8_text`, unchanged -- proper case-folding of non-ASCII text
/// genuinely does need real decoding, so that path's own "only works
/// within a file's own valid-UTF-8 prefix" limitation is accepted as
/// before, just no longer forced onto the ASCII case that didn't need
/// it.
pub(super) fn file_contains_with_chunk_size(path: &Path, needle_lower: &str, chunk_size: usize) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file);
    let mut raw = vec![0u8; chunk_size];
    let first_read = match reader.read(&mut raw) {
        Ok(0) => return false, // empty file, nothing to match
        Ok(n) => n,
        Err(_) => return false,
    };
    if looks_binary(&raw[..first_read]) {
        return false;
    }

    if needle_lower.is_ascii() {
        scan_ascii_bytes(reader, &raw[..first_read], chunk_size, needle_lower.as_bytes())
    } else {
        scan_utf8_text(reader, &raw[..first_read], chunk_size, needle_lower)
    }
}

/// A cheap, approximate "don't bother fully scanning this" check --
/// looks for a null byte anywhere in `chunk` (a candidate's own first
/// read, up to `CONTENT_CHUNK_SIZE`/64 KiB), the same heuristic git and
/// ripgrep both use to skip binary files without a real parse. Added
/// directly after a side-by-side comparison against Far Manager's own
/// content search: a tree with a real mix of source files and binaries
/// (`.exe`, `.dll`, images, ...) used to stream every single one of
/// those binaries through the real scan in full, just to hit an
/// invalid-UTF-8 byte and bail -- often not until well past the first
/// chunk, since compiled binaries frequently have long valid-looking
/// ASCII runs (string tables, padding) before the byte that actually
/// breaks decoding. Reading a real file's whole content just to
/// discover partway through that it was never text to begin with is
/// exactly the wasted work this sniff avoids.
///
/// Not perfect -- a null byte can legitimately appear in some encodings
/// this app doesn't otherwise search anyway (UTF-16, for instance,
/// which `scan_utf8_text` would also reject as invalid UTF-8
/// immediately regardless) -- but cheap, and right in the overwhelmingly
/// common case this app's own file panel already sees: real binaries
/// next to real UTF-8 text, not exotic encodings.
fn looks_binary(chunk: &[u8]) -> bool {
    chunk.contains(&0)
}

/// Raw-byte, encoding-agnostic scan for an ASCII `needle_lower_bytes` --
/// see `file_contains_with_chunk_size`'s own doc comment for why this
/// exists at all. No UTF-8 validation anywhere: every byte is lowercased
/// with `to_ascii_lowercase` (a pure byte-level operation, meaningless
/// notion of "invalid" the way UTF-8 decoding has one) and matched with
/// a plain sliding-window `windows(needle.len()).any(...)` scan.
/// `carry` keeps the trailing `needle.len() - 1` bytes of the previous
/// chunk so a match straddling a chunk boundary is still found, same
/// role `scan_utf8_text`'s own `carry` plays, just over bytes instead of
/// `char`s. `first_chunk` is the read `file_contains_with_chunk_size`
/// already did (and sniffed for binary-ness) before dispatching here --
/// fed into the loop as its own first iteration rather than read again.
fn scan_ascii_bytes(mut reader: impl Read, first_chunk: &[u8], chunk_size: usize, needle_lower_bytes: &[u8]) -> bool {
    let mut raw = vec![0u8; chunk_size];
    let mut carry: Vec<u8> = Vec::new();
    let mut chunk = first_chunk;

    loop {
        carry.extend(chunk.iter().map(u8::to_ascii_lowercase));
        if carry.windows(needle_lower_bytes.len()).any(|window| window == needle_lower_bytes) {
            return true;
        }
        let keep_from = carry.len().saturating_sub(needle_lower_bytes.len().saturating_sub(1));
        if keep_from > 0 {
            carry.drain(..keep_from);
        }

        let read = match reader.read(&mut raw) {
            Ok(0) => return false, // EOF, no match found in any chunk
            Ok(n) => n,
            Err(_) => return false,
        };
        chunk = &raw[..read];
    }
}

/// Correctness across chunk boundaries -- both a multi-byte UTF-8
/// character and the needle itself can straddle two reads:
/// `pending_bytes` carries over any trailing byte sequence that didn't
/// yet decode as a complete character, so it's prepended to (not lost
/// before) the next read; `carry` keeps the lowercased tail of
/// already-decoded text, trimmed back down to roughly the needle's own
/// length after each check, so a match starting a few bytes before a
/// chunk boundary is still seen once the next chunk arrives instead of
/// being split across two independent, non-overlapping searches.
/// `first_chunk` -- see `scan_ascii_bytes`'s own doc comment, same
/// reasoning.
///
/// Only reached for a non-ASCII `needle_lower` now -- see
/// `file_contains_with_chunk_size`'s own doc comment. Still gives up
/// entirely (returns `false`) the moment it hits a genuinely invalid
/// UTF-8 byte sequence anywhere in the file, even if a match was
/// already found in the valid prefix before that point -- that
/// limitation is unchanged from before this file's own encoding-aware
/// split, and is now scoped to the narrower, less common case where a
/// real fix would require actual encoding detection, not just an
/// ASCII-bytes fast path.
fn scan_utf8_text(mut reader: impl Read, first_chunk: &[u8], chunk_size: usize, needle_lower: &str) -> bool {
    let mut raw = vec![0u8; chunk_size];
    let mut pending_bytes: Vec<u8> = Vec::new();
    let mut carry = String::new();
    let needle_chars = needle_lower.chars().count();
    let mut chunk = first_chunk;

    loop {
        pending_bytes.extend_from_slice(chunk);

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

        let read = match reader.read(&mut raw) {
            Ok(0) => return false, // EOF, no match found in any chunk
            Ok(n) => n,
            Err(_) => return false,
        };
        chunk = &raw[..read];
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;
    use std::path::PathBuf;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("find-file-search-content-scan")
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

    /// Regression coverage for the real report (compared side by side
    /// against real Far Manager on the same tree, "1783 vs 1778" -- a
    /// handful of real files silently missing): an ASCII needle
    /// (`"pragma"`) must still be found even when the *rest* of the file
    /// isn't valid UTF-8 -- a legacy source file with e.g.
    /// `Windows-1251`-encoded Cyrillic comments mixed into otherwise
    /// ASCII/UTF-8 content, confirmed by hand as the actual root cause
    /// of one of the missing files (a `#pragma managed(push, off)` line
    /// that used to be silently skipped).
    #[test]
    fn file_contains_finds_an_ascii_match_even_when_the_rest_of_the_file_is_not_valid_utf8() {
        let dir = scratch_dir();
        let path = dir.join("mixed_encoding.cpp");
        let mut content = b"#pragma managed(push, off)\n".to_vec();
        // Windows-1251 bytes for a Cyrillic word -- not valid UTF-8 on
        // their own, simulating a legacy-encoded comment elsewhere in
        // an otherwise-ASCII real source file.
        content.extend_from_slice(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
        fs::write(&path, content).unwrap();

        assert!(file_contains(&path, "pragma"), "an ASCII needle should be found even if the rest of the file isn't valid UTF-8");
    }

    /// Same bug, but the invalid bytes come *before* the match instead
    /// of after -- proves the ASCII byte scan genuinely continues past
    /// non-UTF-8 content rather than happening to work only because the
    /// match sat in the file's own valid-UTF-8 prefix.
    #[test]
    fn file_contains_finds_an_ascii_match_that_comes_after_invalid_utf8_bytes() {
        let dir = scratch_dir();
        let path = dir.join("mixed_encoding_reversed.cpp");
        let mut content = vec![0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        content.extend_from_slice(b"\n#pragma managed(push, off)\n");
        fs::write(&path, content).unwrap();

        assert!(file_contains(&path, "pragma"), "an ASCII needle appearing after non-UTF-8 bytes should still be found");
    }

    #[test]
    fn looks_binary_is_true_for_content_with_a_null_byte() {
        assert!(looks_binary(&[0x41, 0x42, 0x00, 0x43]));
    }

    #[test]
    fn looks_binary_is_false_for_plain_text() {
        assert!(!looks_binary(b"just some ordinary text, no null bytes here"));
    }

    /// `file_contains` should skip a binary file entirely, via the
    /// `looks_binary` sniff over its own first chunk, rather than
    /// needing the real scan to run into invalid UTF-8 partway through
    /// a full streamed pass to reach the same "not a match" conclusion.
    #[test]
    fn file_contains_treats_a_binary_file_as_not_matching_without_a_full_scan() {
        let dir = scratch_dir();
        let path = dir.join("binary.dat");
        // The needle itself is present as real bytes, but preceded by a
        // null byte -- the binary sniff should reject this before the
        // real scan ever gets a chance to find it.
        let mut content = vec![0x00];
        content.extend_from_slice(b"needle");
        fs::write(&path, content).unwrap();

        assert!(!file_contains(&path, "needle"));
    }
}
