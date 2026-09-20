use std::io;
use std::path::PathBuf;

use super::diff::{compute, next_changed_row, previous_changed_row, DiffLineKind};
use super::line_ending::{detect, LineEnding};

/// How many rows `PageUp`/`PageDown` scroll by -- a fixed approximation
/// rather than the real visible pane height (unlike `Panel`, `CompareState`
/// has no feedback loop threading the actual rendered area's height back
/// into it yet -- see `TODO/file-compare.md`'s own "not designed in
/// detail yet" scope note for phase 1). Revisit if this turns out to
/// feel wrong once tried against a real terminal size.
const PAGE_SCROLL_ROWS: usize = 10;

/// One side of the comparison -- its own path, the diff's own display
/// lines and per-row metadata `ui/compare.rs::draw_compare` needs to
/// paint it (`kinds`) and to look up a line-ending marker for it
/// (`source_index`/`line_endings`). Deliberately *not* an
/// `edtui::EditorState` stored here -- `draw_compare` builds one fresh
/// every frame instead, straight from `lines`, optionally with a
/// `CRLF`/`LF` marker appended per line (`App::compare_line_ending_display`,
/// live-toggleable via F9 without reopening the comparison) -- baking a
/// marker into a *persisted* `EditorState`'s own buffer would mean
/// rebuilding it from scratch on every toggle anyway, so there's no
/// actual state worth keeping between frames here.
pub struct ComparePane {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub kinds: Vec<DiffLineKind>,
    /// This row's own index into `line_endings`, or `None` for an
    /// `Empty` padding row -- see `diff::DiffLines::source_index`'s own
    /// doc comment; kept alongside `line_endings` rather than merged
    /// into one `Vec<Option<LineEnding>>` sized to the display rows,
    /// since `line_endings` is naturally sized to the *original* file's
    /// own real line count, not the diff's own padded display row count.
    pub source_index: Vec<Option<usize>>,
    pub line_endings: Vec<Option<LineEnding>>,
}

/// `Alt+F5`'s own full-screen mode: two files, side by side, read-only,
/// GitHub-diff-colored. See `TODO/file-compare.md` for the full design
/// this implements (phase 1 only -- no editing, no 3-way merge).
pub struct CompareState {
    pub left: ComparePane,
    pub right: ComparePane,
    /// The shared vertical scroll row both panes render from --
    /// `ui/compare.rs::draw_compare` applies this to both
    /// `EditorState`s' own `set_viewport_offset` every frame, keeping
    /// them in lockstep (`Panel`'s own left/right panes scroll
    /// independently; this view deliberately doesn't, since the whole
    /// point is comparing the same row on both sides at once).
    pub scroll_row: usize,
}

impl CompareState {
    /// Opens a comparison of `left_path`/`right_path` -- both files are
    /// read whole and diffed once, up front (`diff::compute`), not
    /// incrementally; nothing here handles a file too large to fit
    /// comfortably in memory differently from a small one, matching this
    /// project's other "read the whole file" scope cuts (e.g.
    /// `editor::Editor::open`).
    pub fn open(left_path: PathBuf, right_path: PathBuf) -> io::Result<Self> {
        let left_text = std::fs::read_to_string(&left_path)?;
        let right_text = std::fs::read_to_string(&right_path)?;

        let (left_diff, right_diff) = compute(&left_text, &right_text);
        let left_endings = detect(&left_text);
        let right_endings = detect(&right_text);

        Ok(Self {
            left: ComparePane {
                path: left_path,
                lines: left_diff.lines,
                kinds: left_diff.kinds,
                source_index: left_diff.source_index,
                line_endings: left_endings,
            },
            right: ComparePane {
                path: right_path,
                lines: right_diff.lines,
                kinds: right_diff.kinds,
                source_index: right_diff.source_index,
                line_endings: right_endings,
            },
            scroll_row: 0,
        })
    }

    fn total_rows(&self) -> usize {
        self.left.kinds.len()
    }

    pub fn scroll_up(&mut self, rows: usize) {
        self.scroll_row = self.scroll_row.saturating_sub(rows);
    }

    pub fn scroll_down(&mut self, rows: usize) {
        let max = self.total_rows().saturating_sub(1);
        self.scroll_row = (self.scroll_row + rows).min(max);
    }

    pub fn page_up(&mut self) {
        self.scroll_up(PAGE_SCROLL_ROWS);
    }

    pub fn page_down(&mut self) {
        self.scroll_down(PAGE_SCROLL_ROWS);
    }

    /// `Tab` -- jumps to the next changed row at or after the row just
    /// past the current scroll position, so repeated presses always
    /// make progress instead of finding the same hunk's own first row
    /// again. A no-op past the last hunk.
    pub fn jump_to_next_hunk(&mut self) {
        if let Some(row) = next_changed_row(&self.left.kinds, self.scroll_row + 1) {
            self.scroll_row = row;
        }
    }

    /// `Shift+Tab` -- the other half of `jump_to_next_hunk`.
    pub fn jump_to_previous_hunk(&mut self) {
        if let Some(row) = previous_changed_row(&self.left.kinds, self.scroll_row) {
            self.scroll_row = row;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn open_pair(left_content: &str, right_content: &str) -> CompareState {
        let dir = unique_scratch_dir("compare-state");
        let left_path = dir.join("left.txt");
        let right_path = dir.join("right.txt");
        std::fs::write(&left_path, left_content).unwrap();
        std::fs::write(&right_path, right_content).unwrap();
        CompareState::open(left_path, right_path).unwrap()
    }

    #[test]
    fn open_diffs_both_files_and_starts_scrolled_to_the_top() {
        let state = open_pair("a\nb\nc\n", "a\nx\nc\n");
        assert_eq!(state.scroll_row, 0);
        assert_eq!(state.left.kinds.len(), state.right.kinds.len(), "both panes should always be row-aligned");
    }

    #[test]
    fn scroll_down_is_clamped_at_the_last_row() {
        let mut state = open_pair("a\nb\n", "a\nb\n");
        state.scroll_down(100);
        assert_eq!(state.scroll_row, 1);
    }

    #[test]
    fn scroll_up_is_clamped_at_zero() {
        let mut state = open_pair("a\nb\n", "a\nb\n");
        state.scroll_up(100);
        assert_eq!(state.scroll_row, 0);
    }

    #[test]
    fn jump_to_next_hunk_lands_on_the_first_changed_row_past_the_current_position() {
        let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        state.jump_to_next_hunk();
        assert_eq!(state.scroll_row, 1);
        state.jump_to_next_hunk();
        assert_eq!(state.scroll_row, 3);
        state.jump_to_next_hunk();
        assert_eq!(state.scroll_row, 3, "no further hunk past the last one -- no-op");
    }

    #[test]
    fn jump_to_previous_hunk_lands_on_the_last_changed_row_before_the_current_position() {
        let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        state.scroll_row = 3;
        state.jump_to_previous_hunk();
        assert_eq!(state.scroll_row, 1);
        state.jump_to_previous_hunk();
        assert_eq!(state.scroll_row, 1, "no earlier hunk before the first one -- no-op");
    }
}
