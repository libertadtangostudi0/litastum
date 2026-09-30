/// A name/mask query, parsed once per search instead of per entry.
/// `pattern_chars` feeds the non-ASCII glob fallback; empty for a plain
/// substring query. History: docs/history/find-file-search.md.
pub(super) struct ParsedQuery<'a> {
    lower: &'a str,
    is_glob: bool,
    pattern_chars: Vec<char>,
}

impl<'a> ParsedQuery<'a> {
    /// `lower` must already be lowercased. `pattern_chars` is built for
    /// every glob query, ASCII or not: the `char` path is chosen per
    /// entry, so an ASCII `*.md` still needs it for a non-ASCII name.
    pub(super) fn new(lower: &'a str) -> Self {
        let is_glob = lower.contains('*') || lower.contains('?');
        let pattern_chars = if is_glob { lower.chars().collect() } else { Vec::new() };
        Self { lower, is_glob, pattern_chars }
    }
}

/// `name` arrives here exactly as `entry.file_name()` reported it --
/// **not** pre-lowered by the caller (`walk.rs`) the way it used to be.
/// This function itself owns the case-folding decision, specifically so
/// it can skip allocating a lowered copy of `name` at all for the
/// overwhelmingly common case -- see the ASCII branch's own doc comment
/// below for why that matters.
pub(super) fn matches_query(name: &str, query: &ParsedQuery) -> bool {
    // Allocation-free fast path when both name and query are ASCII
    // (almost always): per-byte case folding. Anything else takes the
    // full-Unicode path.
    if name.is_ascii() && query.lower.is_ascii() {
        if query.is_glob {
            glob_match_ascii(query.lower.as_bytes(), name.as_bytes())
        } else {
            contains_ascii_case_insensitive(name.as_bytes(), query.lower.as_bytes())
        }
    } else {
        let name_lower = name.to_lowercase();
        if query.is_glob {
            glob_match_chars(&query.pattern_chars, &name_lower)
        } else {
            name_lower.contains(query.lower)
        }
    }
}

/// Plain case-insensitive substring search over raw ASCII bytes --
/// `[u8]::eq_ignore_ascii_case` folds case per comparison, so this never
/// needs to build a lowered copy of `haystack` the way
/// `haystack_str.to_lowercase().contains(needle_str)` would.
fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle))
}

/// `glob_match_chars` over ASCII bytes: `pattern` is already lowercase,
/// `text` is folded per byte as it goes -- no allocation.
fn glob_match_ascii(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star_p: Option<usize> = None;
    let mut star_t = 0;

    while t < text.len() {
        let text_byte_lower = text[t].to_ascii_lowercase();
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text_byte_lower) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star_p = Some(p);
            star_t = t;
            p += 1;
        } else if let Some(sp) = star_p {
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|&c| c == b'*')
}

/// Greedy `*`/`?` wildcard match (no `[...]`, no escaping) over `char`s,
/// so a multi-byte name isn't split. Only for non-ASCII input;
/// `glob_match_ascii` is the fast path. `pattern` comes pre-parsed.
fn glob_match_chars(pattern: &[char], text: &str) -> bool {
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

    /// Test-only convenience -- production code always builds `pattern`
    /// once via `ParsedQuery::new`, never per call.
    fn glob_match_chars_str(pattern: &str, text: &str) -> bool {
        let pattern_chars: Vec<char> = pattern.chars().collect();
        glob_match_chars(&pattern_chars, text)
    }

    #[test]
    fn glob_match_chars_examples() {
        assert!(glob_match_chars_str("*.md", "readme.md"));
        assert!(!glob_match_chars_str("*.md", "readme.txt"));
        assert!(glob_match_chars_str("read?e.txt", "readme.txt"));
        assert!(!glob_match_chars_str("read?e.txt", "readmme.txt"), "? is exactly one character, not one-or-more");
        assert!(glob_match_chars_str("*", "anything.at.all"));
        assert!(glob_match_chars_str("a*b*c", "aXXbYYc"));
        assert!(!glob_match_chars_str("a*b*c", "aXXbYY"), "missing the trailing c");
        assert!(glob_match_chars_str("", ""));
        assert!(!glob_match_chars_str("a", ""));
        assert!(glob_match_chars_str("*", ""), "a bare * matches even an empty string");
    }

    #[test]
    fn glob_match_ascii_examples() {
        assert!(glob_match_ascii(b"*.md", b"readme.md"));
        assert!(!glob_match_ascii(b"*.md", b"readme.txt"));
        assert!(glob_match_ascii(b"read?e.txt", b"readme.txt"));
        assert!(!glob_match_ascii(b"read?e.txt", b"readmme.txt"));
        assert!(glob_match_ascii(b"*", b"anything.at.all"));
        assert!(glob_match_ascii(b"a*b*c", b"aXXbYYc"));
        assert!(!glob_match_ascii(b"a*b*c", b"aXXbYY"));
        assert!(glob_match_ascii(b"", b""));
        assert!(!glob_match_ascii(b"a", b""));
        assert!(glob_match_ascii(b"*", b""));
    }

    #[test]
    fn glob_match_ascii_is_case_insensitive_on_the_text_side() {
        assert!(glob_match_ascii(b"*.md", b"README.MD"));
        assert!(glob_match_ascii(b"read?e.txt", b"ReadMe.TXT"));
    }

    #[test]
    fn contains_ascii_case_insensitive_examples() {
        assert!(contains_ascii_case_insensitive(b"README.txt", b"read"));
        assert!(contains_ascii_case_insensitive(b"README.txt", b"README"));
        assert!(!contains_ascii_case_insensitive(b"README.txt", b"nope"));
        assert!(contains_ascii_case_insensitive(b"anything", b""), "an empty needle matches everything");
        assert!(!contains_ascii_case_insensitive(b"short", b"longer than the haystack"));
    }

    #[test]
    fn matches_query_dispatches_to_the_ascii_fast_path_for_plain_ascii_input() {
        assert!(matches_query("README.txt", &ParsedQuery::new("read")));
        assert!(matches_query("README.txt", &ParsedQuery::new("readme.txt")));
        assert!(!matches_query("README.txt", &ParsedQuery::new("nope")));
        assert!(matches_query("readme.md", &ParsedQuery::new("*.md")));
        assert!(!matches_query("readme.txt", &ParsedQuery::new("*.md")));
    }

    /// Regression coverage for the fallback path -- a non-ASCII name or
    /// query should still be matched correctly (full Unicode
    /// case-folding), just via the slower, allocating branch rather than
    /// the byte-level ASCII one.
    #[test]
    fn matches_query_falls_back_correctly_for_non_ascii_names() {
        assert!(matches_query("Österreich.txt", &ParsedQuery::new("österreich")));
        assert!(matches_query("κόσμος.md", &ParsedQuery::new("*.md")));
        assert!(!matches_query("κόσμος.md", &ParsedQuery::new("*.txt")));
    }

    /// Any glob query -- ASCII or not -- should get a real
    /// `pattern_chars`, since `glob_match_chars` can still be reached by
    /// an ASCII query checked against a *non-ASCII name*
    /// (`matches_query_falls_back_correctly_for_non_ascii_names`'s own
    /// `"*.md"` case is exactly this). Only a plain, non-glob query
    /// should leave it empty, since `glob_match_chars` is never reached
    /// at all in that case.
    #[test]
    fn parsed_query_builds_pattern_chars_for_any_glob_query() {
        assert!(!ParsedQuery::new("*.md").pattern_chars.is_empty(), "an ASCII glob query can still be checked against a non-ASCII name");
        assert!(ParsedQuery::new("plain").pattern_chars.is_empty(), "not a glob at all -- glob_match_chars is never reached");
        assert!(!ParsedQuery::new("*.κόσμος").pattern_chars.is_empty(), "a non-ASCII glob query obviously needs it too");
    }
}
