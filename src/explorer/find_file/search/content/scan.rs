use std::fs;
use std::io::Read;
use std::path::Path;

/// Streaming read size -- most text files finish in one or two reads,
/// and a match in a huge file is found without reading far past it.
/// Also the binary-sniff window. Not benchmarked.
const CONTENT_CHUNK_SIZE: usize = 64 * 1024;

/// Whether `path`'s own content contains `needle_lower` (already
/// lowercased by the caller) as a substring, case-insensitively.
pub(super) fn file_contains(path: &Path, needle_lower: &str) -> bool {
    file_contains_with_chunk_size(path, needle_lower, CONTENT_CHUNK_SIZE)
}

/// Streams `path` in chunks and stops at the first match. Opens the file
/// once: the first chunk is both the binary sniff and the scan's first
/// iteration. An ASCII needle is matched on raw bytes, so it's found in
/// any ASCII-compatible encoding (a Windows-1251 file used to be missed);
/// a non-ASCII needle needs real case folding and goes through the UTF-8
/// scan. `chunk_size` is a parameter only so tests can force matches
/// across chunk boundaries. History: docs/history/find-file-search.md.
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

/// A null byte in the first chunk means binary -- git's and ripgrep's
/// heuristic. Skips compiled binaries without streaming them (they often
/// have long ASCII runs before the first invalid byte). Also skips
/// UTF-16, which the UTF-8 scan would reject anyway.
fn looks_binary(chunk: &[u8]) -> bool {
    chunk.contains(&0)
}

/// Encoding-agnostic scan for an ASCII needle: bytes lowercased with
/// `to_ascii_lowercase`, sliding-window match. `carry` keeps the last
/// `needle.len() - 1` bytes so a match across a chunk boundary is found.
/// `first_chunk` is the read the caller already made.
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

/// Scan for a non-ASCII needle. `pending_bytes` carries a character
/// split by the chunk boundary into the next read; `carry` keeps the
/// lowercased tail so a match across the boundary is found. Gives up at
/// the first invalid UTF-8 sequence -- non-ASCII case folding needs real
/// decoding, and encoding detection isn't attempted.
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

    /// An ASCII needle is found in a file that isn't valid UTF-8 elsewhere
    /// (Windows-1251 comments) -- one of the files missing against Far
    /// ("1783 vs 1778"). History: docs/history/find-file-search.md.
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
