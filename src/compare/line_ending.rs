/// Whether the F9 → Compare menu → "Line endings" picker is currently
/// showing a per-line `CRLF`/`LF` marker -- `Hidden` is the default
/// (matching `EditorKeymapMode`/`PopupStyle`'s own "off unless asked
/// for" convention for a setting nothing needed before it existed).
/// Persisted the same way (`theming::config::{load_active_compare_line_ending_display,
/// set_compare_line_ending_display}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LineEndingDisplay {
    #[default]
    Hidden,
    Shown,
}

impl LineEndingDisplay {
    pub fn label(self) -> &'static str {
        match self {
            LineEndingDisplay::Hidden => "Hidden",
            LineEndingDisplay::Shown => "Shown",
        }
    }

    pub fn all() -> [LineEndingDisplay; 2] {
        [LineEndingDisplay::Hidden, LineEndingDisplay::Shown]
    }
}

/// What a single real source line (not a diff's own `Empty` padding
/// row) actually ends in, detected from the file's own raw bytes at
/// load time -- not assumed from the OS or guessed from the *other*
/// lines in the same file. The real reason this feature exists at all:
/// two files can be textually identical yet still genuinely different
/// purely because of a line-ending mismatch (one edited on Windows, one
/// on Unix), which plain syntax-highlighted text can never surface on
/// its own -- `None` is the file's own last line when it has no
/// trailing newline at all, a real, fairly common case (many editors
/// don't force one), not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Crlf,
    Lf,
}

impl LineEnding {
    pub fn marker(self) -> &'static str {
        match self {
            LineEnding::Crlf => " [CRLF]",
            LineEnding::Lf => " [LF]",
        }
    }
}

/// Detects each real line's own terminator directly from `text`,
/// oldest line first -- the file's own last line has `None` if it has
/// no trailing newline at all (see `LineEnding`'s own doc comment).
/// Splits on `\n` by hand rather than `str::lines()` (which already
/// strips `\r`, discarding exactly the distinction this exists to
/// preserve).
pub fn detect(text: &str) -> Vec<Option<LineEnding>> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut endings = Vec::new();
    let mut rest = text;
    loop {
        match rest.find('\n') {
            Some(index) => {
                let ends_crlf = index > 0 && rest.as_bytes()[index - 1] == b'\r';
                endings.push(Some(if ends_crlf { LineEnding::Crlf } else { LineEnding::Lf }));
                rest = &rest[index + 1..];
            }
            None => {
                if !rest.is_empty() {
                    endings.push(None);
                }
                break;
            }
        }
    }
    endings
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_lf_line_endings() {
        assert_eq!(detect("a\nb\n"), vec![Some(LineEnding::Lf), Some(LineEnding::Lf)]);
    }

    #[test]
    fn detects_crlf_line_endings() {
        assert_eq!(detect("a\r\nb\r\n"), vec![Some(LineEnding::Crlf), Some(LineEnding::Crlf)]);
    }

    #[test]
    fn detects_a_mix_of_both_within_the_same_file() {
        assert_eq!(detect("a\r\nb\n"), vec![Some(LineEnding::Crlf), Some(LineEnding::Lf)]);
    }

    #[test]
    fn a_final_line_with_no_trailing_newline_is_none() {
        assert_eq!(detect("a\nb"), vec![Some(LineEnding::Lf), None]);
    }

    #[test]
    fn empty_text_has_no_lines() {
        assert_eq!(detect(""), Vec::<Option<LineEnding>>::new());
    }
}
