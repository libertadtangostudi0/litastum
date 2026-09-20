use std::io;
use std::path::PathBuf;

use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::Index2;

use crate::editor::{Editor, EditorKeymapMode};

use super::diff::{compute, next_changed_row, previous_changed_row};
use super::line_ending::{self, LineEnding};

/// Which pane currently owns the real terminal cursor and receives
/// typed input -- `Tab` toggles this, matching the rest of this app's
/// own convention for switching between two side-by-side panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// `Alt+F5`'s own full-screen mode: two files, side by side, **both
/// fully editable** -- see `TODO/file-compare.md` for the design this
/// implements. Genuinely two plain `Editor`s (undo, syntax highlighting,
/// save, everything `F4` editing already has), not a read-only diff
/// viewer with editing bolted on: `ui/compare.rs` paints GitHub-style
/// red/green backgrounds over them (`Editor::set_extra_highlights`) and
/// keeps the *unfocused* pane's viewport diff-aligned with the focused
/// one (`Editor::set_viewport_top_row`, driven by `diff::map_real_row`
/// every frame) -- but neither pane's real buffer ever has synthetic
/// filler lines injected into it, so saving either one always writes
/// exactly what's actually in the file, nothing more.
pub struct CompareState {
    pub left: Editor,
    pub right: Editor,
    pub focus: Side,
    /// The real cursor position of the pane that currently does *not*
    /// have focus, captured the instant it lost focus -- `None` while
    /// that pane genuinely has never been focused away from since
    /// `open`. Its own `Editor::cursor` is overridden every frame while
    /// unfocused, purely to keep its viewport diff-aligned
    /// (`Editor::set_viewport_top_row`'s own doc comment) -- this is the
    /// only place its true position survives until `toggle_focus`
    /// restores it.
    left_saved_cursor: Option<Index2>,
    right_saved_cursor: Option<Index2>,
    /// Each real line's own `CRLF`/`LF` ending, exactly as it was on
    /// disk when `open` read it (`F9` -> Line endings' `Shown` display,
    /// `ui/compare.rs::draw_line_ending_overlay`). A fixed snapshot, not
    /// re-derived from the live `Editor` buffer on every frame the way
    /// the diff highlights are: `edtui::Lines::from` normalizes `\r\n`
    /// to `\n` the moment a file is loaded (confirmed directly --
    /// `str::lines()`, which it's built on, strips both uniformly), so
    /// the distinction genuinely doesn't survive inside `Editor` at all
    /// -- there is no live buffer this could be recomputed from. A known,
    /// accepted limitation that follows from that: an edit that inserts
    /// or removes lines shifts every later real row's index, so this
    /// snapshot can drift out of sync with which physical line is which
    /// after enough editing -- still meaningfully more useful than
    /// nothing for the common case (checking an unmodified or lightly
    /// edited file for a mixed-line-ending mismatch, the actual reason
    /// this feature exists) than not showing it at all.
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

    /// `Tab`-equivalent "jump to next diff hunk" (bound to `Ctrl+Down`
    /// in `input.rs`, since `Tab` itself now means "switch pane focus" —
    /// see this module's own top doc comment): moves the *focused*
    /// pane's real cursor to the next row with real content that
    /// differs from the other side, diffed live against the other
    /// pane's current text. A no-op past the last hunk.
    pub fn jump_to_next_hunk(&mut self) {
        let focused_kinds = self.live_focused_kinds();
        let from = self.focused().cursor().row + 1;
        if let Some(row) = next_changed_row(&focused_kinds, from) {
            self.focused_mut().set_cursor(Index2::new(row, 0));
        }
    }

    /// `Ctrl+Up` -- the other half of `jump_to_next_hunk`.
    pub fn jump_to_previous_hunk(&mut self) {
        let focused_kinds = self.live_focused_kinds();
        let from = self.focused().cursor().row;
        if let Some(row) = previous_changed_row(&focused_kinds, from) {
            self.focused_mut().set_cursor(Index2::new(row, 0));
        }
    }

    /// The live diff's own row classification for whichever pane is
    /// currently focused, recomputed fresh from both panes' current
    /// text -- shared by the two hunk-jump methods above.
    fn live_focused_kinds(&self) -> Vec<super::diff::DiffLineKind> {
        let (left_diff, right_diff) = compute(&self.left.text(), &self.right.text());
        match self.focus {
            Side::Left => left_diff.kinds,
            Side::Right => right_diff.kinds,
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

    #[test]
    fn jump_to_next_hunk_tracks_whichever_pane_is_focused() {
        let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
        state.toggle_focus();
        state.jump_to_next_hunk();
        assert_eq!(state.right.cursor().row, 1);
        assert_eq!(state.left.cursor().row, 0, "the unfocused pane's cursor shouldn't move");
    }
}
