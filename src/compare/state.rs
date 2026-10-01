use std::io;
use std::path::PathBuf;

use crossterm::event::{MouseEvent, MouseEventKind};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::Index2;

use crate::editor::{Editor, EditorKeymapMode};

use super::diff::{compute, diff_row_of, next_hunk_start, previous_hunk_start, DiffLineKind, DiffLines};
use super::line_ending::{self, LineEnding};

/// Which pane currently owns the real terminal cursor and receives
/// typed input -- `Tab` toggles this, matching the rest of this app's
/// own convention for switching between two side-by-side panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// `Alt+F5`'s screen: two plain, fully editable `Editor`s (undo,
/// highlighting, save). `ui/compare.rs` paints the diff backgrounds and
/// keeps the unfocused pane aligned; no filler lines ever enter a buffer,
/// so saving writes exactly the file. History: docs/history/compare.md.
pub struct CompareState {
    pub left: Editor,
    pub right: Editor,
    pub focus: Side,
    /// The unfocused pane's real cursor, saved when it lost focus: its
    /// `Editor::cursor` is overridden every frame to align its viewport, so
    /// this is where the true position survives until `toggle_focus`.
    left_saved_cursor: Option<Index2>,
    right_saved_cursor: Option<Index2>,
    /// Each real line's ending as read on open -- a snapshot, because
    /// `edtui::Lines::from` normalizes `\r\n` to `\n`, so the live buffer
    /// can't tell. Inserting or deleting lines makes it drift; accepted, since
    /// checking a lightly edited file for mixed endings is the use case.
    left_line_endings: Vec<Option<LineEnding>>,
    right_line_endings: Vec<Option<LineEnding>>,
}

impl CompareState {
    /// Opens a comparison of `left_path`/`right_path` as two ordinary,
    /// independent `Editor` sessions -- `custom_syntax_theme`/`keymap_mode`
    /// are threaded straight through from `App`, the same values `F4`
    /// editing itself already uses, so Compare's panes get identical
    /// syntax highlighting and key bindings to the built-in editor.
    pub fn open(left_path: PathBuf, right_path: PathBuf, custom_syntax_theme: Option<SynTheme>, keymap_mode: EditorKeymapMode) -> io::Result<Self> {
        // Read once, ahead of `Editor::open`'s own read, purely to
        // capture each line's real `CRLF`/`LF` ending before it's lost
        // -- see `left_line_endings`'s own doc comment for why this
        // can't be derived from the `Editor` afterward.
        let left_line_endings = line_ending::detect(&std::fs::read_to_string(&left_path)?);
        let right_line_endings = line_ending::detect(&std::fs::read_to_string(&right_path)?);

        let mut left = Editor::open(left_path, custom_syntax_theme.clone(), keymap_mode)?;
        let mut right = Editor::open(right_path, custom_syntax_theme, keymap_mode)?;
        // Requested directly: per-token syntax coloring competed for
        // attention with the GitHub-style red/green diff backgrounds
        // already painted over changed lines -- plain themed text keeps
        // the diff coloring the one thing drawing the eye here.
        left.disable_syntax_highlighting();
        right.disable_syntax_highlighting();
        Ok(Self { left, right, focus: Side::Left, left_saved_cursor: None, right_saved_cursor: None, left_line_endings, right_line_endings })
    }

    pub fn line_endings(&self, side: Side) -> &[Option<LineEnding>] {
        match side {
            Side::Left => &self.left_line_endings,
            Side::Right => &self.right_line_endings,
        }
    }

    pub fn focused(&self) -> &Editor {
        match self.focus {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    pub fn focused_mut(&mut self) -> &mut Editor {
        match self.focus {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// `Tab` -- hands focus (and the real terminal cursor) to the other
    /// pane, caching the outgoing pane's true cursor position and
    /// restoring the incoming pane's own cached one (see
    /// `left_saved_cursor`/`right_saved_cursor`'s own doc comment).
    pub fn toggle_focus(&mut self) {
        match self.focus {
            Side::Left => {
                self.left_saved_cursor = Some(self.left.cursor());
                if let Some(pos) = self.right_saved_cursor.take() {
                    self.right.set_cursor(pos);
                }
                self.focus = Side::Right;
            }
            Side::Right => {
                self.right_saved_cursor = Some(self.right.cursor());
                if let Some(pos) = self.left_saved_cursor.take() {
                    self.left.set_cursor(pos);
                }
                self.focus = Side::Left;
            }
        }
    }

    /// A mouse event over Compare. A click in the unfocused pane focuses
    /// it first (as `Tab`), so the caret lands where it was clicked;
    /// clicks and drags then go to the focused pane like in the editor.
    /// The wheel scrolls the focused pane wherever the pointer is -- the
    /// other one follows it every frame anyway.
    pub fn mouse(&mut self, mouse: MouseEvent) {
        let over = if self.left.contains_screen_position(mouse.column, mouse.row) {
            Some(Side::Left)
        } else if self.right.contains_screen_position(mouse.column, mouse.row) {
            Some(Side::Right)
        } else {
            None
        };
        let is_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
        if matches!(mouse.kind, MouseEventKind::Down(_)) && over.is_some_and(|side| side != self.focus) {
            self.toggle_focus();
        }
        if is_scroll || over == Some(self.focus) {
            self.focused_mut().mouse(mouse);
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.left.is_dirty() || self.right.is_dirty()
    }

    /// `Ctrl+S` -- saves whichever pane currently has focus, same
    /// single-file-at-a-time convention `F4` editing already has (there
    /// is no "save both" gesture; switch focus and save again for the
    /// other side).
    pub fn save_focused(&mut self) -> io::Result<()> {
        self.focused_mut().save()
    }

    /// `F8`/`Ctrl+Down`: moves the focused pane's cursor to the first row of
    /// the next hunk, diffed live, centered (`Editor::jump_cursor_to`).
    /// Starts from the current row, not `row + 1` (`next_hunk_start`
    /// skips the current hunk). No-op past the last one.
    pub fn jump_to_next_hunk(&mut self) {
        self.jump_to_hunk(next_hunk_start);
    }

    /// `F7`/`Ctrl+Up` -- the other half of `jump_to_next_hunk`, landing
    /// on the previous hunk's own first row (see `previous_hunk_start`'s
    /// own doc comment for why that's not simply the nearest changed
    /// line behind the cursor).
    pub fn jump_to_previous_hunk(&mut self) {
        self.jump_to_hunk(previous_hunk_start);
    }

    /// Runs `find` over the focused pane's live diff and moves the cursor
    /// to the hunk it returns. `find` works in diff rows, which include
    /// `Empty` padding, so the cursor's real row is converted in and the
    /// result back out -- using one as the other put the cursor rows
    /// below the hunk, further the more lines the other side had inserted
    /// above it. History: docs/history/compare.md.
    fn jump_to_hunk(&mut self, find: fn(&[DiffLineKind], usize) -> Option<usize>) {
        let diff = self.live_focused_diff();
        let from = diff_row_of(&diff.source_index, self.focused().cursor().row);
        if let Some(real_row) = find(&diff.kinds, from).and_then(|row| diff.source_index[row]) {
            self.focused_mut().jump_cursor_to(Index2::new(real_row, 0));
        }
    }

    /// The live diff of the focused pane, recomputed from both panes'
    /// current text.
    fn live_focused_diff(&self) -> DiffLines {
        let (left_diff, right_diff) = compute(&self.left.text(), &self.right.text());
        match self.focus {
            Side::Left => left_diff,
            Side::Right => right_diff,
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
        CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap()
    }

    /// Regression coverage: an earlier version detected line endings
    /// from `Editor::text()` at render time, which can never see a
    /// `CRLF` at all -- `edtui::Lines::from` normalizes `\r\n` to `\n`
    /// on load (`str::lines()`, confirmed directly, strips both
    /// uniformly), so `Editor::text()` never contains a `\r` in the
    /// first place. `line_endings` has to come from a real, separate
    /// read of the file's own raw bytes, taken once at `open`.
    #[test]
    fn line_endings_reflect_the_files_own_real_crlf_even_though_the_live_buffer_never_sees_it() {
        let state = open_pair("a\r\nb\n", "a\nb\n");
        assert_eq!(state.left.text(), "a\nb\n", "the live edtui buffer really does lose the \\r on load");
        assert_eq!(state.line_endings(Side::Left), &[Some(LineEnding::Crlf), Some(LineEnding::Lf)]);
        assert_eq!(state.line_endings(Side::Right), &[Some(LineEnding::Lf), Some(LineEnding::Lf)]);
    }

    #[test]
    fn open_starts_focused_on_the_left_pane() {
        let state = open_pair("a\nb\nc\n", "a\nx\nc\n");
        assert_eq!(state.focus, Side::Left);
        assert_eq!(state.focused().text(), "a\nb\nc\n", "Editor::text() should round-trip the file's own content exactly");
    }

    #[test]
    fn toggle_focus_switches_panes_and_preserves_each_sides_own_cursor() {
        let mut state = open_pair("a\nb\nc\n", "x\ny\nz\n");
        state.left.set_cursor(Index2::new(2, 0));
        state.toggle_focus();
        assert_eq!(state.focus, Side::Right);
        assert_eq!(state.right.cursor(), Index2::new(0, 0), "right hasn't been touched yet, still at its own default");

        state.right.set_cursor(Index2::new(1, 0));
        state.toggle_focus();
        assert_eq!(state.focus, Side::Left);
        assert_eq!(state.left.cursor(), Index2::new(2, 0), "left's own cursor should have survived the round trip");

        state.toggle_focus();
        assert_eq!(state.right.cursor(), Index2::new(1, 0), "right's own cursor should also have survived");
    }

    #[test]
    fn is_dirty_reflects_either_pane() {
        let mut state = open_pair("a\n", "b\n");
        assert!(!state.is_dirty());
        state.left.input(crate::test_support::key(crossterm::event::KeyCode::Char('!')));
        assert!(state.is_dirty());
    }

    #[test]
    fn jump_to_next_hunk_moves_the_focused_panes_cursor() {
        let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 1);
        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 3);
        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 3, "no further hunk past the last one -- no-op");
    }

    #[test]
    fn jump_to_previous_hunk_moves_the_focused_panes_cursor_backward() {
        let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        state.left.set_cursor(Index2::new(3, 0));
        state.jump_to_previous_hunk();
        assert_eq!(state.left.cursor().row, 1);
        state.jump_to_previous_hunk();
        assert_eq!(state.left.cursor().row, 1, "no earlier hunk before the first one -- no-op");
    }

    /// Regression coverage for the real report: a multi-line hunk used
    /// to stop `F8`/`jump_to_next_hunk` on every single changed line
    /// inside it before finally moving to the next real hunk -- here,
    /// rows 1-2 on the left are one hunk (both changed), row 4 is a
    /// second, separate one-line hunk. A single press from row 0 should
    /// skip straight past the *entire* first hunk to the second one,
    /// not stop at row 2 first.
    #[test]
    fn jump_to_next_hunk_skips_the_whole_current_hunk_not_one_line_at_a_time() {
        let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nx\ny\nd\nz\n");

        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 1, "should land on the first row of the first (two-line) hunk");

        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 4, "should skip straight past row 2 (still the same hunk) to the second hunk");
    }

    /// The other direction of the same fix: retreating from inside a
    /// multi-line hunk should land on that *previous* hunk's own first
    /// row in one press, not step back through it one line at a time.
    #[test]
    fn jump_to_previous_hunk_skips_the_whole_current_hunk_not_one_line_at_a_time() {
        let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nx\ny\nd\nz\n");
        state.left.set_cursor(Index2::new(4, 0));

        state.jump_to_previous_hunk();
        assert_eq!(state.left.cursor().row, 1, "should land on the first row of the earlier two-line hunk, not row 2 (its last row)");
    }

    /// Pressing "previous" while sitting *inside* the first hunk in the
    /// file (not just on its first row) has nowhere earlier to go --
    /// same "no-op past the boundary" contract
    /// `jump_to_previous_hunk_moves_the_focused_panes_cursor_backward`
    /// already covers for landing exactly on a hunk's own start, just
    /// confirmed here from a row mid-hunk too.
    #[test]
    fn jump_to_previous_hunk_from_mid_first_hunk_is_a_noop() {
        let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nx\ny\nd\nz\n");
        state.left.set_cursor(Index2::new(2, 0)); // second row of the first (rows 1-2) hunk

        state.jump_to_previous_hunk();
        assert_eq!(state.left.cursor().row, 2, "no hunk earlier than the file's own first one -- the cursor shouldn't move at all");
    }

    /// Lines inserted on the right add padding rows to the left's diff, so
    /// diff rows and real rows drift apart. The jump lands on the real row
    /// of the hunk ("d", row 3), not on its diff row (5) -- the cursor
    /// used to end up below the highlighted hunk.
    #[test]
    fn hunk_jumps_land_on_the_real_row_after_padding_rows() {
        let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nnew1\nnew2\nb\nc\nX\ne\n");

        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 3, "\"d\", the replaced line");

        state.jump_to_next_hunk();
        assert_eq!(state.left.cursor().row, 3, "no hunk after it on the left -- the insertion is only padding here");

        state.toggle_focus();
        state.jump_to_next_hunk();
        assert_eq!(state.right.cursor().row, 1, "the insertion on the right");
        state.jump_to_next_hunk();
        assert_eq!(state.right.cursor().row, 5, "\"X\"");
        state.jump_to_previous_hunk();
        assert_eq!(state.right.cursor().row, 1);
    }

    #[test]
    fn jump_to_next_hunk_tracks_whichever_pane_is_focused() {
        let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        state.toggle_focus();
        state.jump_to_next_hunk();
        assert_eq!(state.right.cursor().row, 1);
        assert_eq!(state.left.cursor().row, 0, "the unfocused pane's cursor shouldn't move");
    }
}
