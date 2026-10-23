use std::io;
use std::path::PathBuf;

use crossterm::event::{KeyEvent, MouseEvent, MouseEventKind};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::Index2;

use crate::compare::{hunk_start_rows, CompareState, DiffCache, Side};
use crate::path_edit::{open_failed, PathEdit, PathEditStep};
use crate::editor::{Editor, EditorKeymapMode};

use super::files::ConflictFiles;
use super::markers::{find_conflicts, ConflictRegion};

/// Which of the resolver's panes has focus. The bottom Compare counts as
/// one pane here; it tracks its own left/right focus (`CompareState::focus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Working,
    Result,
    Theirs,
    Incoming,
}

/// Focus stops in `Tab` order: the three top panes, then the bottom
/// Compare's left and right.
const FOCUS_STOPS: usize = 5;

/// The resolver: three plain `Editor`s on top and a `CompareState` of the
/// incoming change below. `.merge-right` is open twice (top right and
/// bottom right) as two independent buffers -- it's SVN's scratch copy,
/// there to read, not to edit.
pub struct ConflictState {
    pub working: Editor,
    pub result: Editor,
    pub theirs: Editor,
    /// `.merge-left` -> `.merge-right`: what the incoming revision changed.
    pub incoming: CompareState,
    pub focus: Pane,
    /// The focused top pane's path title while it's being edited; the
    /// bottom Compare keeps its own (`CompareState::path_edit`).
    pub path_edit: Option<PathEdit>,
    /// Each top pane's real cursor while it's unfocused, by `top_index`:
    /// aligning an unfocused pane overwrites its cursor every frame
    /// (`Editor::set_viewport_top_row`), as in Compare.
    saved_cursors: [Option<Index2>; 3],
    /// `.working` against the result and the result against
    /// `.merge-right`, and the result's conflicts -- redone only when a
    /// text changes, not every frame.
    pub working_diff: DiffCache,
    pub theirs_diff: DiffCache,
    pub conflicts: ConflictsCache,
    /// What the top panes' highlights were last built from (revisions and
    /// colors); `ui/conflict.rs` rebuilds them when it changes.
    pub highlighted: Option<(u64, u64, u64, [ratatui::style::Color; 4])>,
}


/// The result's conflicts, kept until its text changes.
#[derive(Default)]
pub struct ConflictsCache {
    revision: Option<u64>,
    regions: Vec<ConflictRegion>,
}

impl ConflictsCache {
    pub fn get(&mut self, result: &Editor) -> &[ConflictRegion] {
        if self.revision != Some(result.revision()) {
            self.revision = Some(result.revision());
            self.regions = find_conflicts(&result.text());
        }
        &self.regions
    }
}

impl ConflictState {
    /// Opens all five editors; focus starts on the file being resolved.
    /// Syntax colors are off everywhere, as in Compare: diff and conflict
    /// backgrounds are what should draw the eye.
    pub fn open(files: ConflictFiles, custom_syntax_theme: Option<SynTheme>, keymap_mode: EditorKeymapMode) -> io::Result<Self> {
        let mut working = Editor::open(files.working, custom_syntax_theme.clone(), keymap_mode)?;
        let mut result = Editor::open(files.result, custom_syntax_theme.clone(), keymap_mode)?;
        let mut theirs = Editor::open(files.theirs.clone(), custom_syntax_theme.clone(), keymap_mode)?;
        let incoming = CompareState::open(files.base, files.theirs, custom_syntax_theme, keymap_mode)?;
        for editor in [&mut working, &mut result, &mut theirs] {
            editor.disable_syntax_highlighting();
        }
        Ok(Self {
            working,
            result,
            theirs,
            incoming,
            focus: Pane::Result,
            path_edit: None,
            saved_cursors: [None; 3],
            working_diff: DiffCache::default(),
            theirs_diff: DiffCache::default(),
            conflicts: ConflictsCache::default(),
            highlighted: None,
        })
    }

    pub fn focused_mut(&mut self) -> &mut Editor {
        match self.focus {
            Pane::Working => &mut self.working,
            Pane::Result => &mut self.result,
            Pane::Theirs => &mut self.theirs,
            Pane::Incoming => self.incoming.focused_mut(),
        }
    }

    /// `Tab`: the next pane, the bottom Compare's two halves included.
    pub fn focus_next(&mut self) {
        self.set_focus_stop((self.focus_stop() + 1) % FOCUS_STOPS);
    }

    /// `Shift+Tab`: the previous pane.
    pub fn focus_previous(&mut self) {
        self.set_focus_stop((self.focus_stop() + FOCUS_STOPS - 1) % FOCUS_STOPS);
    }

    fn focus_stop(&self) -> usize {
        match self.focus {
            Pane::Working => 0,
            Pane::Result => 1,
            Pane::Theirs => 2,
            Pane::Incoming if self.incoming.focus == Side::Left => 3,
            Pane::Incoming => 4,
        }
    }

    /// Moves focus to `pane`, saving the outgoing top pane's cursor and
    /// restoring the incoming one's (`saved_cursors`).
    fn set_focus(&mut self, pane: Pane) {
        if pane == self.focus {
            return;
        }
        if let (Some(index), Some(editor)) = (top_index(self.focus), self.top_editor(self.focus)) {
            self.saved_cursors[index] = Some(editor.cursor());
        }
        if let Some(position) = top_index(pane).and_then(|index| self.saved_cursors[index].take()) {
            if let Some(editor) = self.top_editor_mut(pane) {
                editor.set_cursor(position);
            }
        }
        self.focus = pane;
    }

    fn set_focus_stop(&mut self, stop: usize) {
        let pane = match stop {
            0 => Pane::Working,
            1 => Pane::Result,
            2 => Pane::Theirs,
            _ => {
                let side = if stop == 3 { Side::Left } else { Side::Right };
                if self.incoming.focus != side {
                    self.incoming.toggle_focus();
                }
                Pane::Incoming
            }
        };
        self.set_focus(pane);
    }

    /// A mouse event: a click focuses the pane under the pointer and
    /// places the caret there; drags go only to the focused pane. The
    /// wheel over the top panes scrolls the focused one, which the others
    /// follow -- scrolling an unfocused one was undone on the next frame
    /// (reported: the wheel sometimes did nothing). The bottom Compare
    /// routes its own halves.
    pub fn mouse(&mut self, mouse: MouseEvent) {
        // An open path field takes its own clicks and drags.
        if self.path_edit.as_mut().is_some_and(|edit| edit.mouse(mouse)) {
            return;
        }
        let Some(pane) = self.pane_at(mouse.column, mouse.row) else {
            return;
        };
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.set_focus(pane);
            // A click anywhere else puts an edited title back.
            self.path_edit = None;
            if pane != Pane::Incoming {
                self.incoming.path_edit = None;
                if self.top_editor(pane).is_some_and(|editor| editor.title_row_contains(mouse.column, mouse.row)) {
                    self.start_path_edit();
                    self.path_edit = self.path_edit.take().map(PathEdit::opened_by_click);
                    return;
                }
            }
        }
        let is_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
        if !is_scroll && pane != self.focus {
            return;
        }
        let pane = if is_scroll && pane != Pane::Incoming && self.focus != Pane::Incoming { self.focus } else { pane };
        match pane {
            Pane::Working => self.working.mouse(mouse),
            Pane::Result => self.result.mouse(mouse),
            Pane::Theirs => self.theirs.mouse(mouse),
            Pane::Incoming => self.incoming.mouse(mouse),
        }
    }

    fn pane_at(&self, column: u16, row: u16) -> Option<Pane> {
        if self.working.contains_screen_position(column, row) {
            Some(Pane::Working)
        } else if self.result.contains_screen_position(column, row) {
            Some(Pane::Result)
        } else if self.theirs.contains_screen_position(column, row) {
            Some(Pane::Theirs)
        } else if self.incoming.left.contains_screen_position(column, row) || self.incoming.right.contains_screen_position(column, row) {
            Some(Pane::Incoming)
        } else {
            None
        }
    }

    /// `F8`/`Alt+Down`: the focused pane's next stop (`stops`), centered.
    /// No-op past the last one; the bottom Compare steps through its hunks.
    pub fn jump_to_next(&mut self) {
        let Some(editor) = self.focused_top_editor() else {
            self.incoming.jump_to_next_hunk();
            return;
        };
        let row = editor.cursor().row;
        if let Some(target) = self.stops(self.focus).into_iter().find(|&stop| stop > row) {
            self.focused_mut().jump_cursor_to(Index2::new(target, 0));
        }
    }

    /// `F7`/`Alt+Up`: the previous stop -- from inside a hunk, its start.
    pub fn jump_to_previous(&mut self) {
        let Some(editor) = self.focused_top_editor() else {
            self.incoming.jump_to_previous_hunk();
            return;
        };
        let row = editor.cursor().row;
        if let Some(target) = self.stops(self.focus).into_iter().rev().find(|&stop| stop < row) {
            self.focused_mut().jump_cursor_to(Index2::new(target, 0));
        }
    }

    fn focused_top_editor(&self) -> Option<&Editor> {
        self.top_editor(self.focus)
    }

    fn top_editor_mut(&mut self, pane: Pane) -> Option<&mut Editor> {
        match pane {
            Pane::Working => Some(&mut self.working),
            Pane::Result => Some(&mut self.result),
            Pane::Theirs => Some(&mut self.theirs),
            Pane::Incoming => None,
        }
    }

    fn top_editor(&self, pane: Pane) -> Option<&Editor> {
        match pane {
            Pane::Working => Some(&self.working),
            Pane::Result => Some(&self.result),
            Pane::Theirs => Some(&self.theirs),
            Pane::Incoming => None,
        }
    }

    /// Where `F7`/`F8` stop in a top pane, ascending: each hunk's start
    /// against its neighbor, as in Compare (`.working` against the result,
    /// `.merge-right` against the result). The result is diffed against
    /// both and also stops on every conflict marker row -- stopping only
    /// on each conflict's `<<<<<<<` skipped too much.
    fn stops(&mut self, pane: Pane) -> Vec<usize> {
        let ((working_side, result_vs_working), _) = self.working_diff.get(&self.working, &self.result);
        let ((result_vs_theirs, theirs_side), _) = self.theirs_diff.get(&self.result, &self.theirs);
        let mut stops = match pane {
            Pane::Working => hunk_start_rows(working_side),
            Pane::Theirs => hunk_start_rows(theirs_side),
            Pane::Result => {
                let mut stops = hunk_start_rows(result_vs_working);
                stops.extend(hunk_start_rows(result_vs_theirs));
                for region in self.conflicts.get(&self.result) {
                    stops.extend([Some(region.start), region.base, Some(region.separator), Some(region.end)].into_iter().flatten());
                }
                stops
            }
            Pane::Incoming => Vec::new(),
        };
        stops.sort_unstable();
        stops.dedup();
        stops
    }

    pub fn is_dirty(&self) -> bool {
        self.working.is_dirty() || self.result.is_dirty() || self.theirs.is_dirty() || self.incoming.is_dirty()
    }

    /// `Ctrl+L` or a click on a title: edit the focused pane's path.
    pub fn start_path_edit(&mut self) {
        match self.focused_top_editor() {
            Some(editor) => self.path_edit = Some(PathEdit::new(editor.path())),
            None => self.incoming.start_path_edit(),
        }
    }

    /// `Shift+F2`: save the focused pane as, through the same field.
    pub fn start_save_as(&mut self) {
        match self.focused_top_editor() {
            Some(editor) => self.path_edit = Some(PathEdit::save_as(editor.path())),
            None => self.incoming.start_save_as(),
        }
    }

    pub fn is_editing_path(&self) -> bool {
        match self.focus {
            Pane::Incoming => self.incoming.path_edit.is_some(),
            _ => self.path_edit.is_some(),
        }
    }

    /// A key while a path field is open; `Err` if the typed path can't be
    /// loaded (the field stays open). As in Compare
    /// (`CompareState::path_edit_key`).
    pub fn path_edit_key(&mut self, key: KeyEvent) -> io::Result<()> {
        if self.focus == Pane::Incoming {
            return self.incoming.path_edit_key(key);
        }
        let Some(edit) = self.path_edit.as_mut() else {
            return Ok(());
        };
        let editor = match self.focus {
            Pane::Working => &mut self.working,
            Pane::Theirs => &mut self.theirs,
            _ => &mut self.result,
        };
        match edit.key_over(key, editor)? {
            PathEditStep::Editing => {}
            PathEditStep::Cancelled | PathEditStep::Saved => self.path_edit = None,
            PathEditStep::Open(path) => {
                self.replace_focused_top(path).map_err(open_failed)?;
                self.path_edit = None;
            }
        }
        Ok(())
    }

    /// Loads `path` into the focused top pane, unless it has unsaved
    /// changes. Highlights and `F7`/`F8` stops follow, being computed from
    /// the live text.
    fn replace_focused_top(&mut self, path: PathBuf) -> io::Result<()> {
        if self.focused_mut().is_dirty() {
            return Err(io::Error::other("this pane has unsaved changes -- save them first (Ctrl+S)"));
        }
        let editor = self.focused_mut().reopen(path)?;
        *self.focused_mut() = editor;
        Ok(())
    }
}



fn top_index(pane: Pane) -> Option<usize> {
    match pane {
        Pane::Working => Some(0),
        Pane::Result => Some(1),
        Pane::Theirs => Some(2),
        Pane::Incoming => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
