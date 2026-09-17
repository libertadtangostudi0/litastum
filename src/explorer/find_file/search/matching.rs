/// `name` and `query_lower` are both assumed already lowercased by the
/// caller (`walk.rs`). Glob semantics only kick in once `query_lower`
/// actually contains a wildcard character.
pub(super) fn matches_query(name: &str, query_lower: &str) -> bool {
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
