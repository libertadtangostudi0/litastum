use edtui::{Index2, Lines};

use super::Editor;

/// One entry in `undo_stack`/`redo_stack` -- a full `Lines` clone plus
/// the cursor position, from immediately before the edit that entry
/// represents ran. The same "brute-force, whole-buffer" shape `edtui`'s
/// own (unreachable) undo stack already uses internally
/// (`edtui::state::undo::UndoState`) -- not reinventing a smarter
/// diff-based scheme, just doing the same thing `edtui` would have,
/// from outside it. See `.claude/rules/litastum-editor-undo.md` for why
/// this stack exists at all instead of `edtui`'s own.
pub(super) struct Snapshot {
    pub(super) lines: Lines,
    pub(super) cursor: Index2,
}


impl Editor {
    /// Pushes a snapshot of the buffer's *current* (pre-edit) state onto
    /// `undo_stack`, capped at `Limits::max_paste_undo_stack` (dropping
    /// the oldest entry once exceeded -- each entry is a full buffer
    /// clone, and this project's own stated scale target is real files
    /// in the tens, sometimes hundreds of thousands of lines, so an
    /// unbounded stack risks real memory growth from a long editing
    /// session). Also clears `redo_stack` -- a fresh edit invalidates
    /// whatever could previously be redone, same convention every other
    /// editor's redo already follows. Called from `input` for every key
    /// that could plausibly mutate the buffer (`should_capture_undo_snapshot`),
    /// and from `paste_text` directly (a fast paste isn't dispatched
    /// through the normal table at all, so it has to trigger its own
    /// capture by hand).
    pub(super) fn push_undo_snapshot(&mut self) {
        self.undo_stack.push(Snapshot { lines: self.state.lines.clone(), cursor: self.state.cursor });
        if self.undo_stack.len() > crate::theming::config::limits().max_paste_undo_stack {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// `Ctrl+Z`: pops the most recent entry off `undo_stack` and
    /// restores it, pushing the state being *left* onto `redo_stack` so
    /// `Ctrl+Y` can bring it back. `false` (no effect) once the stack is
    /// empty -- either nothing mutating has happened yet this session,
    /// or every real edit has already been undone -- `input` then
    /// leaves `Standard`'s own `Ctrl+Z` as a no-op rather than falling
    /// through to `edtui`'s own (entirely separate, never fed by this
    /// keymap) `Undo` action.
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
