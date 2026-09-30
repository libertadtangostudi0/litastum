use edtui::{Index2, Lines};

use super::Editor;

/// One undo/redo entry: a full `Lines` clone and the cursor, from just
/// before the edit -- the same shape as `edtui`'s own (unreachable) stack.
/// Why ours: `.claude/rules/litastum-editor-undo.md`.
pub(super) struct Snapshot {
    pub(super) lines: Lines,
    pub(super) cursor: Index2,
}


impl Editor {
    /// Pushes the current (pre-edit) state, capped at
    /// `Limits::max_paste_undo_stack` (oldest dropped -- each entry is a full
    /// buffer), and clears `redo_stack`. Called by `input` for mutating keys
    /// and by `paste_text`, which bypasses the table.
    pub(super) fn push_undo_snapshot(&mut self) {
        self.undo_stack.push(Snapshot { lines: self.state.lines.clone(), cursor: self.state.cursor });
        if self.undo_stack.len() > crate::theming::config::limits().max_paste_undo_stack {
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

    /// Restores `snapshot` as the live buffer/cursor and returns the
    /// state it replaced -- the shared half of `undo`/`redo`, which only
    /// differ in which stack each side comes from and goes to.
    fn swap_in(&mut self, snapshot: Snapshot) -> Snapshot {
        let current = Snapshot { lines: std::mem::replace(&mut self.state.lines, snapshot.lines), cursor: self.state.cursor };
        self.state.cursor = snapshot.cursor;
        self.buffer_changed();
        current
    }
}
