/// A field's text, cursor and selection anchor at one point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub text: String,
    pub cursor: usize,
    pub anchor: Option<usize>,
}

/// Oldest steps are dropped past this; a single-line field never needs more.
const MAX_STEPS: usize = 100;

/// `Ctrl+Z`/`Ctrl+Y` for a `TextField`. A run of typed characters is one
/// step, as in an editor; any other edit, or a cursor move, ends the run.
#[derive(Debug, Clone, Default)]
pub(super) struct UndoHistory {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing_run: bool,
}

impl UndoHistory {
    /// An edit changed the text; `before` is the state it started from.
    pub fn record(&mut self, before: Snapshot, is_typing: bool) {
        if !(is_typing && self.typing_run) {
            self.undo.push(before);
            if self.undo.len() > MAX_STEPS {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.typing_run = is_typing;
    }

    /// A key that didn't change the text: the next typed character starts
    /// a new step.
    pub fn break_run(&mut self) {
        self.typing_run = false;
    }

    /// The state to go back to from `current`, if any.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.typing_run = false;
        Some(previous)
    }

    /// The state an undo left, from `current`, if any.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.typing_run = false;
        Some(next)
    }
}
