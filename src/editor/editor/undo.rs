use edtui::{Index2, Lines, RowIndex};

use super::Editor;

/// What an undo/redo entry restores.
pub(super) enum Change {
    /// The whole buffer -- for edits that can add or remove rows (Enter,
    /// paste, deleting a selection).
    Buffer(Lines),
    /// One row's content -- typing, Backspace and Delete within a row,
    /// the overwhelmingly common edits. A full clone on every keystroke
    /// cost ~170 ms on a 300k-line file even in a release build.
    Row { row: usize, content: Vec<char> },
}

/// One undo/redo entry: what to restore and the cursor, from just before
/// the edit. Why ours, not `edtui`'s: `.claude/rules/litastum-editor-undo.md`.
pub(super) struct Snapshot {
    pub(super) change: Change,
    pub(super) cursor: Index2,
}

impl Snapshot {
    pub(super) fn buffer(lines: Lines, cursor: Index2) -> Self {
        Self { change: Change::Buffer(lines), cursor }
    }
}


/// How many undo steps are kept at most. One-row entries are small; the
/// whole-buffer ones have their own, much lower cap.
const MAX_UNDO_ENTRIES: usize = 1000;


fn buffer_entries(stack: &[Snapshot]) -> usize {
    stack.iter().filter(|snapshot| matches!(snapshot.change, Change::Buffer(_))).count()
}


impl Editor {
    /// Pushes the whole current (pre-edit) buffer -- see `Change`.
    pub(super) fn push_undo_snapshot(&mut self) {
        let snapshot = Snapshot::buffer(self.state.lines.clone(), self.state.cursor);
        self.push_snapshot(snapshot);
    }

    /// Pushes just `row` as it is before an edit that changes only it.
    /// Correct because entries are restored newest first: when this one
    /// comes back, every later edit has been undone, so only `row` differs
    /// from the state before this edit.
    pub(super) fn push_row_undo_snapshot(&mut self, row: usize) {
        let content = self.state.lines.get(RowIndex::new(row)).cloned().unwrap_or_default();
        let snapshot = Snapshot { change: Change::Row { row, content }, cursor: self.state.cursor };
        self.push_snapshot(snapshot);
    }

    /// Oldest entries are dropped past `MAX_UNDO_ENTRIES`, or past
    /// `Limits::max_paste_undo_stack` whole-buffer copies; a new edit
    /// clears `redo_stack`.
    fn push_snapshot(&mut self, snapshot: Snapshot) {
        self.undo_stack.push(snapshot);
        let max_buffers = crate::theming::config::limits().max_paste_undo_stack;
        while self.undo_stack.len() > MAX_UNDO_ENTRIES || buffer_entries(&self.undo_stack) > max_buffers {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// `Ctrl+Z`: restores the newest entry and moves the current state to
    /// `redo_stack`. `false` when empty; `Standard`'s `Ctrl+Z` is then a
    /// no-op, never `edtui`'s `Undo`.
    pub(super) fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.swap_in(snapshot);
        self.redo_stack.push(current);
        true
    }

    /// `Ctrl+Y`: the mirror of `undo` -- pops `redo_stack`, restores it,
    /// and pushes the state being left back onto `undo_stack`. `false`
    /// (no effect) once there's nothing left to redo.
    pub(super) fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.swap_in(snapshot);
        self.undo_stack.push(current);
        true
    }

    /// Restores `snapshot` and returns the same kind of entry for the
    /// state it replaced -- the shared half of `undo`/`redo`.
    fn swap_in(&mut self, snapshot: Snapshot) -> Snapshot {
        let cursor = std::mem::replace(&mut self.state.cursor, snapshot.cursor);
        let change = match snapshot.change {
            Change::Buffer(lines) => {
                let old = std::mem::replace(&mut self.state.lines, lines);
                self.buffer_changed();
                Change::Buffer(old)
            }
            Change::Row { row, content } => {
                let old = match self.state.lines.get_mut(RowIndex::new(row)) {
                    Some(current) => std::mem::replace(current, content),
                    None => Vec::new(),
                };
                self.row_changed(row);
                // Several rows differed, so the row check alone can't
                // tell whether this undo made the buffer clean; an undo
                // is rare enough for the full comparison.
                if self.differing == super::changes::Differing::Elsewhere {
                    self.buffer_changed();
                }
                Change::Row { row, content: old }
            }
        };
        Snapshot { change, cursor }
    }
}
