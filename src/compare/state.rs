use std::io;
use std::path::PathBuf;

use crossterm::event::{MouseEvent, MouseEventKind};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::Index2;

use crate::editor::{Editor, EditorKeymapMode};

use super::diff::{diff_row_of, next_hunk_start, previous_hunk_start, DiffLineKind};
use super::diff_cache::DiffCache;
use super::line_ending::{self, LineEnding};
use crate::path_edit::{open_failed, PathEdit, PathEditStep};

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
    pub(crate) left_line_endings: Vec<Option<LineEnding>>,
    pub(crate) right_line_endings: Vec<Option<LineEnding>>,
    /// The focused pane's path title while it's being edited.
    pub path_edit: Option<PathEdit>,
    /// The panes' diff, redone only when a text changes.
    pub diff: DiffCache,
    /// The colors the row highlights were last built with; a theme
    /// change rebuilds them though the diff didn't change.
    pub highlight_colors: Option<(ratatui::style::Color, ratatui::style::Color)>,
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
        Ok(Self { left, right, focus: Side::Left, left_saved_cursor: None, right_saved_cursor: None, left_line_endings, right_line_endings, path_edit: None, diff: DiffCache::default(), highlight_colors: None })
    }

    #[cfg(test)]
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
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            let title = if self.left.title_row_contains(mouse.column, mouse.row) {
                Some(Side::Left)
            } else if self.right.title_row_contains(mouse.column, mouse.row) {
                Some(Side::Right)
            } else {
                None
            };
            if let Some(side) = title {
                if side != self.focus {
                    self.toggle_focus();
                }
                self.start_path_edit();
                return;
            }
            // A click anywhere else puts the title back.
            self.path_edit = None;
        }
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

    /// `Ctrl+L` or a click on the title: edit the focused pane's path.
    pub fn start_path_edit(&mut self) {
        self.path_edit = Some(PathEdit::new(self.focused().path()));
    }

    /// `Shift+F2`: the focused pane's title as a field to save it as.
    pub fn start_save_as(&mut self) {
        self.path_edit = Some(PathEdit::save_as(self.focused().path()));
    }

    /// A key while the path field is open. `Err` if the typed path can't
    /// be loaded or saved to -- the field stays open to fix it; the error
    /// reads as a whole notice.
    pub fn path_edit_key(&mut self, key: crossterm::event::KeyEvent) -> io::Result<()> {
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok(());
        };
        let editor = match self.focus {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        };
        match edit.key_over(key, editor)? {
            PathEditStep::Editing => {}
            PathEditStep::Cancelled | PathEditStep::Saved => self.path_edit = None,
            PathEditStep::Open(path) => {
                self.replace_focused(path).map_err(open_failed)?;
                self.path_edit = None;
            }
        }
        Ok(())
    }

    /// Loads `path` into the focused pane; the diff follows on the next
    /// frame, since it's recomputed from the live text. Refused while the
    /// pane has unsaved changes, which would be lost.
    pub fn replace_focused(&mut self, path: PathBuf) -> io::Result<()> {
        if self.focused().is_dirty() {
            return Err(io::Error::other("this pane has unsaved changes -- save them first (Ctrl+S)"));
        }
        let line_endings = line_ending::detect(&std::fs::read_to_string(&path)?);
        let editor = self.focused().reopen(path)?;
        match self.focus {
            Side::Left => (self.left, self.left_line_endings) = (editor, line_endings),
            Side::Right => (self.right, self.right_line_endings) = (editor, line_endings),
        }
        Ok(())
    }

    /// `Ctrl+S` -- saves whichever pane currently has focus, same
    /// single-file-at-a-time convention `F4` editing already has (there
    /// is no "save both" gesture; switch focus and save again for the
    /// other side).
    pub fn save_focused(&mut self) -> io::Result<()> {
        self.focused_mut().save()
    }

    /// `F8`/`Alt+Down`: moves the focused pane's cursor to the first row of
    /// the next hunk, diffed live, centered (`Editor::jump_cursor_to`).
    /// Starts from the current row, not `row + 1` (`next_hunk_start`
    /// skips the current hunk). No-op past the last one.
    pub fn jump_to_next_hunk(&mut self) {
        self.jump_to_hunk(next_hunk_start);
    }

    /// `F7`/`Alt+Up` -- the other half of `jump_to_next_hunk`, landing
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
        let cursor_row = self.focused().cursor().row;
        let ((left_diff, right_diff), _) = self.diff.get(&self.left, &self.right);
        let diff = match self.focus {
            Side::Left => left_diff,
            Side::Right => right_diff,
        };
        let from = diff_row_of(&diff.source_index, cursor_row);
        if let Some(real_row) = find(&diff.kinds, from).and_then(|row| diff.source_index[row]) {
            self.focused_mut().jump_cursor_to(Index2::new(real_row, 0));
        }
    }
}


#[cfg(test)]
mod tests;
