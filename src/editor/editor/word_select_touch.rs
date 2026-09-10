use edtui::EditorMode;

use super::Editor;

/// See `Editor::word_select_touch`'s own doc comment for why this
/// exists. Three states, not two (`Option<bool>`/"has it gone forward
/// yet"), because a plain boolean can't tell "word-wise selection has
/// never touched this selection at all" (`Untouched` -- e.g. it was
/// built by character-wise `Shift+Right`, a mouse drag, or anything
/// else that isn't `extend_word_selection`) apart from "word-wise
/// selection built this whole thing itself via repeated backward
/// presses" (`NativeBackward`) -- confirmed the hard way: a selection
/// built by *anything other than* word-wise `Right` presses (real
/// report: a whole line selected some other way, then trimmed with
/// `Ctrl+Shift+Left`) needs the *same* full retraction `Untouched`
/// wants, but an `Option<bool>` collapsing both of those into one value
/// can't tell them apart from `NativeBackward`'s own "keep walking
/// backward through nothing already selected" case, which must *not*
/// retract (`repeated_left_monotonically_extends_through_punctuation`,
/// unaffected on purpose).
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
    // Known, narrow gap, not chased further: `Editor::extend_word_selection`
    // never resets this back to `Untouched` -- once any word-wise press
    // happens, it's `Touched`/`NativeBackward` for good, since there's no
    // hook here for "the selection was closed and a *different* one was
    // built some other way" (that happens entirely inside `Editor::input`,
    // outside this type's view). In practice this only matters if an
    // earlier word-wise session ended in `NativeBackward` *and* a later,
    // entirely separate selection (built without word-wise selection ever
    // touching it) is then retracted with `Ctrl+Shift+Left` as its very
    // first action -- `Touched` left over instead gives the right answer
    // anyway, since `Touched` and `Untouched` both retract; only a leftover
    // `NativeBackward` would wrongly skip it. Narrow enough (two unrelated
    // things have to line up) not to be worth a bigger hook for yet.
}


impl Editor {
    /// `Ctrl+Shift+Left`/`Right` -- word-wise selection. Not part of
    /// `input`'s own dispatch (`editor_keymap.rs::handle_editor_key`
    /// calls this directly instead) -- see `bindings::extend_word_selection`'s
    /// own doc comment for why this needed real logic of its own rather
    /// than another entry in `standard_key_handler`'s declarative table.
    ///
    /// Owns `word_select_touch` -- reads it (as `retracing`, "should
    /// this press give back territory toward the anchor rather than
    /// extend past it") before this press changes anything, then
    /// updates it for next time. Two symmetric cases, one per
    /// direction:
    ///
    /// - A backward (`!forward`) press against an existing selection
    ///   (`!fresh`) that isn't a pure `NativeBackward` walk -- both
    ///   `Untouched` (word-wise selection has never acted on it -- built
    ///   some other way, or this is the very first backward touch of
    ///   it) and `Touched` (word-wise selection has gone forward, or
    ///   already retracted, at least once) count, which is exactly what
    ///   lets both real reports in `extend_word_selection`'s own doc
    ///   comment ("Eighth") retract correctly -- one starting from a
    ///   word-wise `Right`-built selection, the other from one built
    ///   some other way entirely.
    /// - A forward (`forward`) press against an existing selection
    ///   (`!fresh`) that *is* a pure `NativeBackward` walk -- the mirror
    ///   case added for `extend_word_selection`'s own "Fourteenth"
    ///   report (`Ctrl+Shift+Right` undoing a selection built purely by
    ///   `Ctrl+Shift+Left`).
    ///
    /// `word_select_touch` stays `NativeBackward` across a *retracing*
    /// forward press (not just across backward ones) -- deliberately,
    /// so a second `Right` (or a `Left` right after a `Right`) still
    /// gets treated as mirroring the same backward walk instead of
    /// falling through to `Touched`'s own broader rule after only one
    /// retracing press. Without this, `retracing`'s own forward
    /// condition above (`touch == NativeBackward`, narrower than the
    /// backward condition's `touch != NativeBackward`) would stop
    /// firing after the very first `Right`, and a second one would hit
    /// the ordinary forward branch instead -- which, depending on
    /// exactly where the anchor and the current word boundary happen to
    /// line up, can still land in the right place by coincidence but
    /// leaves a stray one-character selection sitting on the anchor
    /// rather than closing cleanly. A *genuine* forward press (i.e. not
    /// retracing -- extending past the anchor into text the backward
    /// walk never covered) still becomes `Touched`, same as before.
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
