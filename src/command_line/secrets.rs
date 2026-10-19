//! Secrets in a typed command -- a password in a URL, after `--password`,
//! in `DB_PASSWORD=...` -- kept out of what litastum stores or shows again
//! (requested: passwords never go to the history or the log). A command
//! with one isn't added to the history (saved with `***` it would run
//! wrong when recalled); the log and the user screen get it redacted.


/// The keys whose value is a secret, compared lowercase with `-` as `_`:
/// these as a whole key or its last part (`db_password`, `GITHUB_TOKEN`).
const SECRET_KEYS: &[&str] = &["password", "passwd", "passphrase", "token", "secret", "apikey", "api_key"];

/// Short keys that are secrets only on their own (`--pass`, `-pwd`), not
/// as a part (`bypass`).
const SHORT_SECRET_KEYS: &[&str] = &["pass", "pwd"];

const MASK: &str = "***";


/// Whether `line` holds a secret (`redact` would change it).
pub fn has_secret(line: &str) -> bool {
    redact(line) != line
}


/// `line` with its secrets as `***`: a URL's password (`://user:***@`),
/// the value of a secret key (`--password=***`, `TOKEN=***`), and the
/// word after a secret flag (`--password ***`, `-Password ***`).
pub fn redact(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut mask_next = false;
    for piece in split_keeping_spaces(line) {
        if piece.trim().is_empty() {
            out.push_str(piece);
            continue;
        }
        if mask_next {
            out.push_str(MASK);
            mask_next = false;
            continue;
        }
        let word = redact_url_password(piece);
        match split_key_value(&word) {
            Some((key, _)) if is_secret_key(key) => {
                out.push_str(&word[..key.len() + 1]);
                out.push_str(MASK);
            }
            Some(_) => out.push_str(&word),
            None => {
                mask_next = is_flag(&word) && is_secret_key(&word);
                out.push_str(&word);
            }
        }
    }
    out
}


/// `line` cut into words and the runs of spaces between them, in order.
fn split_keeping_spaces(line: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (at, c) in line.char_indices() {
        let space = c.is_whitespace();
        if in_space.is_some_and(|was| was != space) {
            pieces.push(&line[start..at]);
            start = at;
        }
        in_space = Some(space);
    }
    if start < line.len() {
        pieces.push(&line[start..]);
    }
    pieces
}


/// `scheme://user:password@host` -> `scheme://user:***@host`.
fn redact_url_password(word: &str) -> String {
    let Some(scheme_end) = word.find("://") else {
        return word.to_string();
    };
    let authority_start = scheme_end + 3;
    let authority_end = word[authority_start..].find(['/', '?', '#']).map_or(word.len(), |at| authority_start + at);
    let authority = &word[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return word.to_string();
    };
    let Some(colon) = authority[..at].find(':') else {
        return word.to_string();
    };
    format!("{}{}{MASK}{}", &word[..authority_start], &authority[..=colon], &word[authority_start + at..])
}


/// `key=value` or `key:value` (`/p:secret`), the key with its leading
/// dashes, slashes or `$env:` -- quotes around the value stay part of it.
fn split_key_value(word: &str) -> Option<(&str, &str)> {
    if word.contains("://") {
        return None;
    }
    let at = word.find(['=', ':'])?;
    let (key, value) = (&word[..at], &word[at + 1..]);
    if key.is_empty() || key.eq_ignore_ascii_case("$env") {
        // `$env:TOKEN="x"` -- the key is after the colon.
        let (inner, value) = value.split_once('=')?;
        return Some((&word[..at + 1 + inner.len()], value));
    }
    Some((key, value))
}


fn is_flag(word: &str) -> bool {
    word.starts_with('-') || word.starts_with('/')
}


fn is_secret_key(key: &str) -> bool {
    let name = key.trim_start_matches(['-', '/', '$']).trim_start_matches("env:").to_lowercase().replace('-', "_");
    let name = name.trim_matches(['"', '\'']);
    SHORT_SECRET_KEYS.contains(&name)
        || SECRET_KEYS.iter().any(|secret| name == *secret || name.ends_with(&format!("_{secret}")) || (name.ends_with(secret) && secret.len() >= 6))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_urls_password_is_masked_and_the_rest_kept() {
        assert_eq!(redact("hg pull https://me:s3cr3t@hg.example.com/repo"), "hg pull https://me:***@hg.example.com/repo");
        assert_eq!(redact("git clone https://me@github.com/x.git"), "git clone https://me@github.com/x.git", "no password: as is");
    }

    #[test]
    fn a_secret_flag_or_key_has_its_value_masked() {
        assert_eq!(redact("svn up --username me --password s3cr3t"), "svn up --username me --password ***");
        assert_eq!(redact("tool --password=s3cr3t --verbose"), "tool --password=*** --verbose");
        assert_eq!(redact("Connect -Password s3cr3t"), "Connect -Password ***");
        assert_eq!(redact("set DB_PASSWORD=s3cr3t"), "set DB_PASSWORD=***");
        assert_eq!(redact("$env:GITHUB_TOKEN=\"abc\""), "$env:GITHUB_TOKEN=***");
        assert_eq!(redact("curl -H x --api-key abc"), "curl -H x --api-key ***");
    }

    #[test]
    fn ordinary_commands_are_left_alone() {
        for line in ["dir /s", "hg pull", "cargo test --bin litastum", "git log --pretty=oneline", "echo bypass", "copy a.txt b.txt", "cd W:\\work"] {
            assert_eq!(redact(line), line);
            assert!(!has_secret(line), "{line}");
        }
    }

    #[test]
    fn spaces_between_words_are_kept() {
        assert_eq!(redact("a   --token  x  b"), "a   --token  ***  b");
    }
}
