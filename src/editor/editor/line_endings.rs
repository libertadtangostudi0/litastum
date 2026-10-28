//! A file's line breaks: read when it's opened, shown on the editor's
//! bottom border (`CRLF`, `LF`, `Mixed`), written back by a save, and
//! changed only on request (F9 -> Line endings). Reported: a save wrote
//! `\n` into a CRLF file. History: docs/history/editor-saving.md.

use super::Editor;


/// A line break a file can be saved with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub const ALL: [Self; 2] = [Self::Lf, Self::CrLf];

    fn platform() -> Self {
        if cfg!(windows) {
            Self::CrLf
        } else {
            Self::Lf
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }

    /// Its name on the border.
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
        }
    }

    /// Its name in the F9 picker.
    pub fn description(self) -> &'static str {
        match self {
            Self::Lf => "LF (Unix, macOS)",
            Self::CrLf => "CRLF (Windows)",
        }
    }
}


/// The line breaks a file has on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineEndings {
    /// One kind throughout -- or no line break at all, then the
    /// platform's.
    Uniform(LineEnding),
    /// Both kinds; `dominant` is the one most of them are.
    Mixed { dominant: LineEnding },
}

impl LineEndings {
    pub(super) fn of(text: &str) -> Self {
        let breaks = text.matches('\n').count();
        let crlf = text.matches("\r\n").count();
        match (crlf, breaks - crlf) {
            (0, 0) => Self::Uniform(LineEnding::platform()),
            (_, 0) => Self::Uniform(LineEnding::CrLf),
            (0, _) => Self::Uniform(LineEnding::Lf),
            (crlf, lf) => Self::Mixed { dominant: if crlf > lf { LineEnding::CrLf } else { LineEnding::Lf } },
        }
    }

    fn dominant(self) -> LineEnding {
        match self {
            Self::Uniform(ending) | Self::Mixed { dominant: ending } => ending,
        }
    }
}


/// The file's line breaks and the one picked for it, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LineEndingState {
    on_disk: LineEndings,
    chosen: Option<LineEnding>,
}

impl LineEndingState {
    pub(super) fn of(text: &str) -> Self {
        Self { on_disk: LineEndings::of(text), chosen: None }
    }

    /// What a save writes: the one picked, else the file's own (most of
    /// them, in a mixed file).
    pub(super) fn to_write(self) -> LineEnding {
        self.chosen.unwrap_or(self.on_disk.dominant())
    }

    /// Whether a save would change the file's line breaks on request.
    pub(super) fn changed(self) -> bool {
        self.chosen.is_some_and(|chosen| self.on_disk != LineEndings::Uniform(chosen))
    }

    fn label(self) -> &'static str {
        match (self.chosen, self.on_disk) {
            (None, LineEndings::Mixed { .. }) => "Mixed",
            _ => self.to_write().label(),
        }
    }

    /// The file as just written.
    pub(super) fn saved(&mut self) {
        *self = Self { on_disk: LineEndings::Uniform(self.to_write()), chosen: None };
    }
}


impl Editor {
    /// The line break a save writes -- the F9 picker marks it.
    pub fn line_ending(&self) -> LineEnding {
        self.line_endings.to_write()
    }


    /// What the bottom border says: `CRLF`, `LF`, or `Mixed` for a file
    /// with both, until one is picked.
    pub fn line_ending_label(&self) -> &'static str {
        self.line_endings.label()
    }


    /// F9 -> Line endings: the next save writes `ending` throughout. A
    /// change of the file's own, so it makes the editor modified -- and
    /// picking the file's own again doesn't.
    pub fn set_line_ending(&mut self, ending: LineEnding) {
        self.line_endings.chosen = Some(ending);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_kind_throughout_is_uniform() {
        assert_eq!(LineEndings::of("a\r\nb\r\n"), LineEndings::Uniform(LineEnding::CrLf));
        assert_eq!(LineEndings::of("a\nb\n"), LineEndings::Uniform(LineEnding::Lf));
        assert_eq!(LineEndings::of("a\nb"), LineEndings::Uniform(LineEnding::Lf), "no break after the last line");
    }

    #[test]
    fn both_kinds_are_mixed_with_the_more_common_one_dominant() {
        assert_eq!(LineEndings::of("a\r\nb\r\nc\n"), LineEndings::Mixed { dominant: LineEnding::CrLf });
        assert_eq!(LineEndings::of("a\nb\nc\r\n"), LineEndings::Mixed { dominant: LineEnding::Lf });
        assert_eq!(LineEndings::of("a\r\nb\n"), LineEndings::Mixed { dominant: LineEnding::Lf }, "a tie");
    }

    #[test]
    fn a_text_with_no_line_break_gets_the_platforms() {
        let expected = LineEndings::Uniform(if cfg!(windows) { LineEnding::CrLf } else { LineEnding::Lf });
        assert_eq!(LineEndings::of("one line"), expected);
        assert_eq!(LineEndings::of(""), expected);
        assert_eq!(LineEndings::of("a\rb"), expected, "a lone CR is no line break");
    }

    #[test]
    fn the_label_says_mixed_until_one_is_picked() {
        let mut state = LineEndingState::of("a\r\nb\n\nc\n");
        assert_eq!(state.label(), "Mixed");
        assert_eq!(state.to_write(), LineEnding::Lf);
        state.chosen = Some(LineEnding::CrLf);
        assert_eq!(state.label(), "CRLF");
        assert!(state.changed());
    }

    #[test]
    fn picking_the_files_own_changes_nothing() {
        let mut state = LineEndingState::of("a\r\nb\r\n");
        state.chosen = Some(LineEnding::CrLf);
        assert!(!state.changed());
        state.chosen = Some(LineEnding::Lf);
        assert!(state.changed());
    }

    #[test]
    fn picking_the_dominant_one_of_a_mixed_file_is_still_a_change() {
        let mut state = LineEndingState::of("a\r\nb\r\nc\n");
        state.chosen = Some(LineEnding::CrLf);
        assert!(state.changed(), "the LF line becomes CRLF");
    }

    #[test]
    fn a_save_makes_the_written_one_the_files() {
        let mut state = LineEndingState::of("a\r\nb\n");
        state.chosen = Some(LineEnding::CrLf);
        state.saved();
        assert_eq!(state, LineEndingState { on_disk: LineEndings::Uniform(LineEnding::CrLf), chosen: None });
        assert!(!state.changed());
        assert_eq!(state.label(), "CRLF");
    }
}
