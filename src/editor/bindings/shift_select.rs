use crossterm::event::KeyCode;
use edtui::actions::{Execute, MoveBackward, MoveForward, SwitchMode};
use edtui::{EditorMode, EditorState, Index2};

/// Reported directly: pressing `Shift+Right` once right before "Draft"
/// selected two characters, `"Dr"`, not one, `"D"`. Traced to
/// `standard_key_handler`'s own fresh-selection table entries (the
/// `i(...)` registrations for `Shift+Left`/`Right`), which used to
/// chain `SwitchMode(Visual)` with the matching `Move` action in a
/// single step -- but `SwitchMode(Visual)` alone already anchors a
/// selection on the *current* cell (vim's own `v` semantics: entering
/// visual mode with no movement yet already "selects" the character
/// under the cursor), so chaining a `Move` on top grabbed a *second*
/// character for what a user experiences as one keypress. This is
/// exactly the "N shift-rights selects N+1 characters, not N" quirk
/// documented in `.claude/rules/litastum-stack.md` -- previously
/// accepted as an unavoidable side effect of reusing `edtui`'s own vim-
/// style actions, but the project's own stated goal is a "standard
/// (non-modal, VSCode/Windows-convention) keymap," where one keypress
/// should mean one character, not two.
///
/// **Scoped to `Left`/`Right` only -- deliberately not `Up`/`Down`.** A
/// first attempt applied the exact same fix to all four directions,
/// and was reported broken immediately: `Shift+Down` started selecting
/// only one character to the *right*, not moving to the next line at
/// all, with the second press then landing one column off. `Up`/`Down`
/// never actually had the "N+1, not N" bug in the first place --
/// there's no single-character granularity to get right or wrong for a
/// row jump, unlike `Left`/`Right`. The *wanted* behavior for
/// `Shift+Down` -- move to the same column on the next line,
/// extending the selection to cover everything in between -- is
/// exactly what the *original*, unmodified chain
/// (`SwitchMode(Visual).chain(MoveDown(1))`, still in
/// `bindings/mod.rs`'s table for `Up`/`Down`) already produced. Only
/// `Left`/`Right`'s fresh entries were changed to drop the chained
/// `Move` (see their own comment in `standard_key_handler`); `Up`/
/// `Down` keep it, unmodified, and never reach this function's own
/// per-direction logic below -- they fall through the `_ => true` arm,
/// a harmless no-op, since `wrap_line_boundary_arrow_movement` (the
/// only thing this function's return value gates) never acts on
/// `Up`/`Down` either.
///
/// **First attempt at `Left`/`Right` itself** treated both the same
/// way: if `SwitchMode(Visual)`'s own anchor cell (`cursor_before`)
/// held a real character, stop there (one character selected);
/// otherwise fall back to performing the direction's own `Move`. This
/// is correct for `Right` -- the anchor cell itself, the one the
/// cursor is currently *on*, is exactly the character a forward
/// selection should grab first. **Wrong for `Left`, reported
/// immediately**: `Shift+Left` selected (and copied) the character to
/// the *right* of the cursor, not the left. The anchor cell is never
/// the right answer for a *backward* selection -- the character a
/// `Shift+Left` press should select is the one the cursor is about to
/// move *onto* (one cell further left), not the one it's currently
/// sitting on. Stopping on `cursor_before` for `Left` was really just
/// repeating the original "N+1, not N" bug in the opposite direction,
/// wearing the "fix" as a disguise.
///
/// **Landed on**: split `Right` and `Left` by which way they actually
/// travel. `Right` keeps the anchor-only behavior above -- unmoved and
/// correct, since the current cell genuinely is the first one a
/// forward selection should include. `Left` always performs its `Move`
/// first (there's no valid anchor-only answer for it at all), then --
/// if that move made real progress -- drags the anchor
/// (`selection.start`, still sitting wherever `SwitchMode(Visual)`
/// first planted it, on `cursor_before`) to match the *new* cursor
/// position too, collapsing the selection down to exactly the one cell
/// just moved onto rather than spanning `[new, old]` (two cells, the
/// same shape of bug all over again). If the move made *no* progress
/// at all (already at the very start of the line/buffer), the anchor
/// is left untouched and this function reports "not yet handled,"
/// handing it to `wrap_line_boundary_arrow_movement` -- exactly like a
/// plain, non-fresh `Left` press already does when it hits a line
/// boundary, so crossing into the previous line on a fresh
/// `Shift+Left` still works, same as `Right`'s own already-tested
/// symmetric case.
///
/// Returns `true` when this press is already fully resolved (`Right`
/// stopped on a real character, or `Left` moved and dragged the
/// anchor) -- `Editor::input` uses this to skip the line-boundary wrap
/// check entirely in that case: a zero-movement fresh selection
/// anchored on a real character (or a real already-completed backward
/// move) is a deliberate stop, not a signal that a plain arrow press
/// hit a wall, and treating it as one would wrongly wrap an ordinary
/// mid-line press down into the next line. Returns `false` when the
/// wrap check should still get a look -- `Right`'s own fallback-move
/// case (might have hit a real boundary), or `Left`'s zero-progress
/// case (might need to wrap to the previous line). `Up`/`Down` always
/// return `true` (see their own doc comment below) -- harmless either
/// way, since `wrap_line_boundary_arrow_movement` never acts on them.
///
/// **`Up`/`Down` needed the same "exclude one column" treatment too,
/// after a real report against aligned text** (two lines with a word
/// landing on the identical column on both): one `Shift+Down` from
/// right before that word swept the destination row's own copy of it
/// into the selection too, since `MoveDown`/`MoveUp` (confirmed
/// directly from `edtui`'s source) only ever change `state.cursor.row`,
/// never `.col` -- the destination genuinely lands on the exact same
/// column as the anchor. A first fix (trimming the destination's/
/// anchor's own edge column by one, mirroring `Right`'s own
/// single-character logic, mutating `state.cursor.col`/
/// `selection.start.col` directly) shipped -- **then broke round-trip
/// symmetry, reported immediately**: `Shift+Down` then `Shift+Up` (or
/// the reverse) no longer returned to an empty selection at the exact
/// starting point. Root cause: `MoveDown`/`MoveUp` never re-derive
/// `.col` from anything, they just carry whatever's already in
/// `state.cursor` forward -- so the one-time column adjustment from the
/// first press permanently "poisoned" the column for every future
/// vertical move in *either* direction, including the reversing one
/// that's supposed to land exactly back on the anchor.
///
/// **Reverted first, then re-added with the missing piece**: the
/// column adjustment alone has nowhere to record what the column was
/// *before* it got trimmed -- `Up`'s own half of it already mutates
/// `selection.start` itself, so by the time a reversing press needs to
/// snap back to the real anchor, `selection.start` no longer reliably
/// records where the selection actually began. The fix landed once
/// there was somewhere else to keep that value: `Editor` now owns
/// `vertical_shift_anchor_col: Option<usize>`, set once (to
/// `cursor_before.col`, always identical on both ends the instant
/// `SwitchMode(Visual)` opens a selection -- before either edge has
/// been trimmed) whenever a fresh `Shift+Up`/`Down` press opens a
/// selection, alongside the actual column trim
/// (`exclude_landing_column_on_fresh_vertical_selection`, below).
/// `close_selection_if_back_on_the_anchors_row` reads that tracked
/// value back once the excursion is over and restores it to
/// `state.cursor.col`, so the trimmed, "poisoned" column never survives
/// past the press that closes the selection -- letting the aligned-word
/// exclusion and perfect round-trip symmetry coexist, instead of having
/// to pick one.
pub(in crate::editor) fn anchor_fresh_shift_selection(state: &mut EditorState, key_code: KeyCode, cursor_before: Index2) -> bool {
    match key_code {
        KeyCode::Right => forward_anchor(state, cursor_before, MoveForward(1)),
        KeyCode::Left => backward_anchor(state, cursor_before, MoveBackward(1)),
        _ => true,
    }
}

/// Trims the aligned landing column out of a freshly-opened `Shift+Up`/
/// `Down` selection -- see `anchor_fresh_shift_selection`'s own doc
/// comment for the real report this fixes and why it only runs once,
/// on the press that opens the selection (`Editor::input` calls this
/// only when `freshly_entered_visual` and the key is `Up`/`Down`;
/// `MoveUp`/`MoveDown` never touch `.col` themselves, so whatever this
/// leaves it at simply carries forward unchanged on every later
/// continuing press).
///
/// `Down`: the destination row (`state.cursor`, kept in lock-step with
/// `selection.end` per this codebase's own cursor-equals-selection-end
/// invariant) backs off one column so its own row's selected span stops
/// right before that column. `Up`: it's `selection.start` (the row the
/// press started on, now the selection's *bottom* edge, never touched
/// by `MoveUp` itself) that needs the trim instead -- confirmed against
/// `edtui`'s own multi-line selection convention that whichever raw
/// `Selection` field sits on the larger row is also an inclusive upper
/// bound on that row's own selected span, rather than assumed from the
/// single-row case. Column 0 has nothing to trim into (no adjustment,
/// same as the destination/anchor genuinely starting at the very
/// beginning of its own line) -- left untouched rather than
/// underflowing.
pub(in crate::editor) fn exclude_landing_column_on_fresh_vertical_selection(state: &mut EditorState, key_code: KeyCode) {
    match key_code {
        KeyCode::Down => {
            if state.cursor.col == 0 {
                return;
            }
            state.cursor.col -= 1;
            if let Some(selection) = state.selection.as_mut() {
                selection.end = state.cursor;
            }
        }
        KeyCode::Up => {
            if let Some(selection) = state.selection.as_mut() {
                if selection.start.col > 0 {
                    selection.start.col -= 1;
                }
            }
        }
        _ => {}
    }
}

/// Called unconditionally from `Editor::input` for every `Shift+Up`/
/// `Down` press (fresh *and* continuing -- unlike the two functions
/// above, which only ever run once, on the press that opens a
/// selection). If the cursor has landed back on the exact row the
/// selection's own anchor (`selection.start`) sits on, there's nothing
/// left of this vertical excursion to show as selected -- closes it
/// entirely, the same "can't represent empty, so close it instead" move
/// `word_select.rs`'s "Tenth"/"Eleventh" fixes already use for
/// word-wise selection. Also restores `state.cursor.col` from
/// `true_anchor_col` first (`Editor::vertical_shift_anchor_col`,
/// cleared here once used) -- without this, the fresh-press column trim
/// `exclude_landing_column_on_fresh_vertical_selection` applies would
/// permanently leave the cursor one column short of where it actually
/// started, since `MoveUp`/`MoveDown` never re-derive `.col` on their
/// own to correct it back. A no-op for every other key, and for
/// `Up`/`Down` themselves whenever nothing is selected yet or the
/// cursor is still on a genuinely different row.
pub(in crate::editor) fn close_selection_if_back_on_the_anchors_row(
    state: &mut EditorState,
    key_code: KeyCode,
    true_anchor_col: &mut Option<usize>,
) {
    if !matches!(key_code, KeyCode::Up | KeyCode::Down) {
        return;
    }
    let Some(selection) = state.selection.as_ref() else {
        return;
    };
    if state.cursor.row != selection.start.row {
        return;
    }
    if let Some(col) = true_anchor_col.take() {
        state.cursor.col = col;
    }
    SwitchMode(EditorMode::Normal).execute(state);
    SwitchMode(EditorMode::Insert).execute(state);
}

/// `Right`: the anchor cell (where the cursor already sits) is exactly
/// the first character a forward selection should include -- stop
/// there if it's real, otherwise fall back to actually performing the
/// move (nothing valid to anchor on, e.g. the append position past a
/// line's last character).
fn forward_anchor(state: &mut EditorState, cursor_before: Index2, mut move_action: impl Execute) -> bool {
    if state.lines.get(cursor_before).is_some() {
        return true;
    }
    move_action.execute(state);
    false
}

/// `Left`: there's no valid anchor-only answer -- the character to
/// select is always the one the cursor is about to move *onto*, never
/// the one it's currently sitting on. Always performs the move first;
/// if that made real progress, drags the anchor to match the new
/// cursor position too (collapsing the selection to exactly that one
/// cell, instead of spanning old-to-new). If the move made no progress
/// at all, leaves the anchor untouched and reports "not yet handled" so
/// the line-boundary wrap check gets a chance to cross into the
/// previous line instead.
fn backward_anchor(state: &mut EditorState, cursor_before: Index2, mut move_action: impl Execute) -> bool {
    move_action.execute(state);
    if state.cursor == cursor_before {
        return false;
    }
    if let Some(selection) = state.selection.as_mut() {
        selection.start = state.cursor;
    }
    true
}


#[cfg(test)]
mod tests {
    use edtui::actions::{Execute, SwitchMode};
    use edtui::{EditorMode, EditorState, Lines};

    use super::{
        anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    };

    /// Uses the real `SwitchMode(Visual)` action (not a raw
    /// `state.mode = ...` field assignment) specifically so `edtui`
    /// itself constructs the `Selection` it always creates as a side
    /// effect of entering `Visual` mode -- its type is `pub(crate)`
    /// (unreachable to name directly, per this file's own project
    /// history), so this is the only way to get a real one, and it's
    /// also exactly what the real table entry now does before this
    /// function ever runs.
    ///
    /// `state.mode` is set to `Insert` *before* `cursor.col`, matching
    /// the real pipeline (a fresh Shift+arrow always fires while still
    /// in `Insert`) -- `SwitchMode`'s own `execute` calls
    /// `state.clamp_column()` using whatever mode is current *before*
    /// switching, and `edtui`'s default (pre-`Insert`) mode clamps to
    /// `len - 1`, not `len` -- found the hard way when a test using the
    /// line's own append position (`col == len`, nothing real there)
    /// got silently clamped back onto a real character before this
    /// function ever ran, making the test assert the wrong thing
    /// entirely.
    fn state_for(contents: &str, cursor_col: usize) -> EditorState {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.cursor.col = cursor_col;
        SwitchMode(EditorMode::Visual).execute(&mut state);
        state
    }

    #[test]
    fn right_stops_immediately_on_a_real_character() {
        let mut state = state_for("Draft", 0);
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Right, cursor_before);
        assert!(handled);
        assert_eq!(state.cursor.col, 0, "should not have moved at all -- the anchor cell alone is the whole selection");
    }

    #[test]
    fn right_falls_back_to_a_real_move_at_the_append_position() {
        let mut state = state_for("hi", 2); // one past 'i', nothing real there
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Right, cursor_before);
        assert!(!handled, "nothing real to anchor on -- should have fallen back to an actual move");
    }

    /// Regression test for the real report: `Shift+Left` was selecting
    /// (and copying) the character to the *right* of the cursor, not the
    /// left -- the anchor-only behavior correct for `Right` was being
    /// reused unmodified for `Left`, where it's simply the wrong cell.
    #[test]
    fn left_selects_the_character_actually_to_the_left() {
        let mut state = state_for("Draft", 2); // cursor on 'a', the third letter
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Left, cursor_before);

        assert!(handled, "real progress was made -- this press is fully resolved, no wrap check needed");
        assert_eq!(state.cursor.col, 1, "should have moved onto 'r', the character actually to the left");
        let selection = state.selection.expect("should still have a selection");
        assert_eq!(selection.start, state.cursor, "the anchor should have been dragged to match -- exactly one cell selected");
        assert_eq!(selection.end, state.cursor);
    }

    /// `Shift+Left` with nothing before the cursor at all (column 0)
    /// must not fabricate a selection out of thin air -- reports "not
    /// yet handled" so the line-boundary wrap check (or, at the very
    /// start of the whole buffer, simply nothing) can decide instead.
    #[test]
    fn left_at_the_very_start_of_a_line_reports_unhandled() {
        let mut state = state_for("Draft", 0);
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Left, cursor_before);
        assert!(!handled, "no progress was possible -- must not claim this press is resolved");
        assert_eq!(state.cursor.col, 0, "should not have moved");
    }

    /// Direct unit coverage for the actual fix, isolated from the
    /// `MoveUp`/`MoveDown` actions and the rest of `Editor::input`'s
    /// pipeline (see `bindings/tests.rs` for the full round-trip
    /// integration tests through real key presses). Simulates "cursor
    /// landed back on the anchor's own row" by hand -- exactly the
    /// state a pure vertical excursion leaves behind, since
    /// `MoveUp`/`MoveDown` never touch `.col`. No `true_anchor_col`
    /// tracked here (`None`) -- covers the "nothing to restore" case;
    /// see `restores_the_true_anchor_column_once_closed` below for the
    /// tracked-column case this exists alongside `Editor`'s own field
    /// for.
    #[test]
    fn closes_the_selection_once_the_cursor_is_back_on_the_anchors_row() {
        let mut state = state_for("Draft", 1);
        let anchor_row = state.selection.as_ref().expect("SwitchMode(Visual) should anchor a selection").start.row;
        state.cursor.row = anchor_row;

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Down, &mut None);

        assert!(state.selection.is_none(), "should have collapsed the selection entirely");
        assert_eq!(state.mode, EditorMode::Insert);
    }

    #[test]
    fn leaves_the_selection_alone_while_still_on_a_different_row() {
        let mut state = state_for("Draft", 1);
        state.cursor.row = 5; // nowhere near the anchor's own row

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Down, &mut None);

        assert!(state.selection.is_some(), "should still be selecting -- the excursion isn't over yet");
    }

    #[test]
    fn is_a_no_op_for_keys_other_than_up_or_down() {
        let mut state = state_for("Draft", 1);
        let anchor_row = state.selection.as_ref().expect("SwitchMode(Visual) should anchor a selection").start.row;
        state.cursor.row = anchor_row;

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Right, &mut None);

        assert!(state.selection.is_some(), "Right/Left have their own handling -- this function must not touch them");
    }

    /// Regression coverage for the round-trip fix's own missing piece:
    /// a fresh vertical selection trims `state.cursor.col` by one (see
    /// `exclude_landing_column_on_fresh_vertical_selection`), and
    /// without restoring it here, the cursor would permanently end up
    /// one column short of where it actually started once the
    /// selection closes.
    #[test]
    fn restores_the_true_anchor_column_once_closed() {
        let mut state = state_for("terminal one\nterminal two", 4); // between 't' and 'e'
        let mut true_anchor_col = Some(state.cursor.col);
        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down);
        assert_eq!(state.cursor.col, 3, "should have trimmed the landing column by one");

        let anchor_row = state.selection.as_ref().unwrap().start.row;
        state.cursor.row = anchor_row; // simulate the reversing Up press landing back here

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Up, &mut true_anchor_col);

        assert_eq!(state.cursor.col, 4, "should be back on the exact column the excursion started from");
        assert!(true_anchor_col.is_none(), "should have consumed the tracked value");
    }

    /// `Down`: the destination row's own copy of whatever sits on the
    /// aligned column should be excluded -- see
    /// `anchor_fresh_shift_selection`'s doc comment for the real report.
    #[test]
    fn down_trims_the_destination_rows_landing_column() {
        let mut state = state_for("terminal one\nterminal two", 4);
        state.cursor.row = 1; // as if MoveDown(1) already ran
        if let Some(selection) = state.selection.as_mut() {
            selection.end.row = 1;
        }

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down);

        assert_eq!(state.cursor.col, 3);
        assert_eq!(state.selection.unwrap().end.col, 3, "selection.end must stay in lock-step with the cursor");
    }

    /// `Up`: the trim applies to `selection.start` instead (the row the
    /// press started on, now the selection's own bottom edge) --
    /// `state.cursor` itself is untouched, since `MoveUp` lands it on a
    /// different row than the one being trimmed.
    #[test]
    fn up_trims_the_anchors_own_starting_column() {
        let mut state = state_for("terminal one\nterminal two", 4);
        state.cursor.row = 1; // press started on row 1
        if let Some(selection) = state.selection.as_mut() {
            selection.start.row = 1;
        }
        state.cursor.row = 0; // as if MoveUp(1) already ran

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Up);

        assert_eq!(state.selection.unwrap().start.col, 3);
        assert_eq!(state.cursor.col, 4, "MoveUp never touches .col -- this function must not either, for Up");
    }

    /// Column 0 has nothing to trim into -- must not underflow.
    #[test]
    fn makes_no_adjustment_at_column_zero() {
        let mut state = state_for("terminal one\nterminal two", 0);
        state.cursor.row = 1;
        if let Some(selection) = state.selection.as_mut() {
            selection.end.row = 1;
        }

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down);

        assert_eq!(state.cursor.col, 0);
    }
}
