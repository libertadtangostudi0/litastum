use edtui::EditorMode;

use super::Editor;

/// What word-wise selection has done to the current selection. Three
/// states, because a selection built some other way (`Untouched`) needs
/// the same retraction as `Touched`, while a pure backward walk
/// (`NativeBackward`) must not retract. History: docs/history/word-select.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WordSelectTouch {
    /// No selection at all, or one exists but word-wise selection
    /// hasn't acted on it yet. A backward press from here should
    /// retract fully -- there's real content to shrink away from,
    /// word-wise selection just hasn't touched it before.
    Untouched,
    /// The *current* selection was built by word-wise selection itself,
    /// starting with a backward (`Left`) press, and every press since
    /// has also been backward -- a pure walk backward through fresh
    /// text extending the selection, not retracting anything.
    NativeBackward,
    /// Word-wise selection has done at least one forward (`Right`)
    /// press on the current selection, or at least one retraction --
    /// a backward press from here is undoing part of that.
    Touched,
    // Never reset to `Untouched` -- a known, narrow gap (see the history).
}


impl Editor {
    /// `Ctrl+Shift+Left`/`Right` -- word-wise selection, called from
    /// `editor_keymap` rather than `input`'s table. Reads
    /// `word_select_touch` as `retracing` before acting, then updates it:
    ///
    /// - backward on an existing selection that isn't a pure
    ///   `NativeBackward` walk retracts toward the anchor;
    /// - forward on a `NativeBackward` walk retracts it (the mirror).
    ///
    /// A retracing forward press keeps `NativeBackward`, so the next one
    /// still mirrors the walk; a genuine forward press becomes `Touched`.
    /// History: docs/history/word-select.md.
    pub fn extend_word_selection(&mut self, forward: bool) {
        let fresh = self.state.mode != EditorMode::Visual;
        let retracing = if forward {
            !fresh && self.word_select_touch == WordSelectTouch::NativeBackward
        } else {
            !fresh && self.word_select_touch != WordSelectTouch::NativeBackward
        };

        crate::editor::bindings::extend_word_selection(&mut self.state, forward, retracing, &mut self.word_select_true_anchor);

        self.word_select_touch = match (fresh, forward, retracing, self.word_select_touch) {
            (true, true, _, _) => WordSelectTouch::Touched,
            (true, false, _, _) => WordSelectTouch::NativeBackward,
            (false, true, true, _) => WordSelectTouch::NativeBackward,
            (false, true, false, _) => WordSelectTouch::Touched,
            (false, false, _, WordSelectTouch::NativeBackward) => WordSelectTouch::NativeBackward,
            (false, false, _, _) => WordSelectTouch::Touched,
        };
    }
}
