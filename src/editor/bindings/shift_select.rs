use crossterm::event::KeyCode;
use edtui::actions::{Execute, MoveBackward, MoveForward};
use edtui::{EditorState, Index2};

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
/// **`Up`/`Down`, a separate report against the same fresh-selection
/// mechanism.** Unlike `Left`/`Right`, `Up`/`Down`'s table entries were
/// never touched by any of the fixes above -- they still chain
/// `SwitchMode(Visual)` with their own `Move` (`bindings/mod.rs`'s own
/// comment explains why: there's no single-character granularity to
/// get right or wrong for a row jump). Reported anyway, against real
/// text with two lines aligned so a word landed on the identical column
/// on both: one `Shift+Down` from right before that word on the first
/// line selected one row too much -- the *destination* row's own copy
/// of that word came along too (`"editor.rs    — L"` instead of the
/// wanted `"editor.rs    — "`). `MoveDown`/`MoveUp` (confirmed directly
/// from `edtui`'s source) only ever change `state.cursor.row` -- they
/// never touch `.col` at all, so the destination cell really does land
/// on the exact same column as the anchor, and `edtui`'s own
/// inclusive-both-ends model includes whatever's there, same as every
/// other cell in the range.
///
/// For `Down`, the anchor (the cell the press started on) is correctly
/// included -- same as `Right`'s own "the current cell is the first one
/// a forward selection should grab" answer -- but the *landing* cell on
/// the new row shouldn't be, mirroring `Right`'s own single-character
/// convention applied to the *far* end of a multi-row span instead of a
/// single cell: back the cursor (and `selection.end`, kept in lock-step
/// per the `cursor == selection.end` invariant everywhere else in this
/// codebase) off by one column, so the destination row's own selected
/// span stops right before that column instead of on it.
///
/// For `Up`, it's the mirror image: the press's own *starting* cell (now
/// the far/bottom end of the selection, sitting in `selection.start`,
/// untouched by `MoveUp`) is the one that shouldn't be included --
/// exactly `Left`'s own "the cell the cursor departs from is never the
/// right answer for a backward selection" logic, just applied to the
/// anchor instead of the cursor. `selection.start`/`.end` aren't sorted
/// by row -- whichever field sits on the *larger* row is the selection's
/// own bottom edge, and (confirmed against `edtui`'s own already-tested
/// multi-line behavior, not just reasoned from the single-row case)
/// that edge's own column is *also* an inclusive upper bound on that
/// row, same direction as `Down`'s `state.cursor` -- so excluding column
/// `cursor_before.col` there means nudging `selection.start.col`
/// *back* by one too, not forward (an earlier version of this got the
/// direction backwards and made things worse, including one character
/// too many instead of one too few). Doesn't touch `state.cursor` at
/// all -- the destination (now `selection.end`, at the top) already
/// landed exactly right, matching `Down`'s own anchor-side answer,
/// nothing to adjust there.
///
/// Both adjustments only ever run once, on the fresh press that opens
/// the selection (guarded the same way as the rest of this function,
/// via `Editor::input`'s own `freshly_entered_visual` check) --
/// `MoveDown`/`MoveUp` never touch `.col` on their own, so whatever this
/// leaves `state.cursor.col` (for `Down`) or `selection.start.col` (for
/// `Up`) at simply carries forward unchanged on every further
/// *continuing* press, without needing to be re-applied or drifting
/// further with each additional row.
pub(in crate::editor) fn anchor_fresh_shift_selection(state: &mut EditorState, key_code: KeyCode, cursor_before: Index2) -> bool {
    match key_code {
        KeyCode::Right => forward_anchor(state, cursor_before, MoveForward(1)),
        KeyCode::Left => backward_anchor(state, cursor_before, MoveBackward(1)),
        KeyCode::Down => exclude_the_destination_rows_own_landing_column(state, cursor_before),
        KeyCode::Up => exclude_the_anchor_rows_own_starting_column(state, cursor_before),
        _ => true,
    }
}

/// See `anchor_fresh_shift_selection`'s own "Up/Down" doc section. The
/// table already performed the real row change (`SwitchMode(Visual)
/// .chain(MoveDown(1))`) before this runs -- this only trims the
/// landing column back by one, and only when a row change actually
/// happened (a `Shift+Down` on the very last line makes no progress at
/// all, nothing to trim).
fn exclude_the_destination_rows_own_landing_column(state: &mut EditorState, cursor_before: Index2) -> bool {
    if state.cursor.row == cursor_before.row || state.cursor.col == 0 {
        return true;
    }
    state.cursor.col -= 1;
    if let Some(selection) = state.selection.as_mut() {
        selection.end = state.cursor;
    }
    true
}

/// See `anchor_fresh_shift_selection`'s own "Up/Down" doc section.
/// Nudges `selection.start` back by one column, the same direction
/// `exclude_the_destination_rows_own_landing_column` nudges `state.cursor`
/// -- both are shrinking their own row's contribution to the selection
/// by excluding its trailing/leading edge column, just on opposite
/// fields since `Up`'s anchor sits on the *bottom* row this time
/// instead of the top. `state.cursor` already landed on the correct
/// destination cell (`MoveUp` already ran as part of the table's own
/// chain) and is left untouched.
fn exclude_the_anchor_rows_own_starting_column(state: &mut EditorState, cursor_before: Index2) -> bool {
    if state.cursor.row == cursor_before.row || cursor_before.col == 0 {
        return true;
    }
    if let Some(selection) = state.selection.as_mut() {
        if selection.start == cursor_before {
            selection.start.col -= 1;
        }
    }
    true
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

    use super::anchor_fresh_shift_selection;

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

    /// Builds multi-line state and replicates the table's own chain for
    /// `Up`/`Down` (`SwitchMode(Visual).chain(MoveDown(1))` or
    /// `MoveUp(1)`) before `anchor_fresh_shift_selection` ever runs --
    /// unlike `Left`/`Right`'s fresh table entries, which no longer
    /// chain any `Move` at all, `Up`/`Down` still do, so this function
    /// always receives an already-moved state to correct, never a
    /// freshly-anchored, unmoved one.
    fn state_after_row_move(contents: &str, cursor_row: usize, cursor_col: usize, key_code: crossterm::event::KeyCode) -> (EditorState, edtui::Index2) {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.cursor.row = cursor_row;
        state.cursor.col = cursor_col;
        let cursor_before = state.cursor;
        SwitchMode(EditorMode::Visual).execute(&mut state);
        match key_code {
            crossterm::event::KeyCode::Down => edtui::actions::MoveDown(1).execute(&mut state),
            crossterm::event::KeyCode::Up => edtui::actions::MoveUp(1).execute(&mut state),
            _ => unreachable!("only Up/Down are used with this helper"),
        }
        (state, cursor_before)
    }

    /// Regression test for the real report against real, aligned text:
    /// one `Shift+Down` right before a word that lands on the identical
    /// column on the next line too must not sweep that next line's own
    /// copy of the word into the selection.
    #[test]
    fn down_excludes_the_destination_rows_own_landing_column() {
        let (mut state, cursor_before) = state_after_row_move("aaa LATER one\nbbb LATER two", 0, 4, crossterm::event::KeyCode::Down);
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Down, cursor_before);

        assert!(handled);
        assert_eq!(state.cursor, edtui::Index2 { row: 1, col: 3 }, "should land one column short of the aligned word");
        let selection = state.selection.expect("should still have a selection");
        assert_eq!(selection.start, cursor_before, "the anchor (top row) should stay exactly where the press started");
        assert_eq!(selection.end, state.cursor);
    }

    /// Mirror-image regression test for `Shift+Up`: the press's own
    /// starting row (now the selection's bottom edge) must exclude its
    /// own copy of the word, while the destination (top) row keeps it
    /// in full.
    #[test]
    fn up_excludes_the_anchor_rows_own_starting_column() {
        let (mut state, cursor_before) = state_after_row_move("aaa LATER one\nbbb LATER two", 1, 4, crossterm::event::KeyCode::Up);
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Up, cursor_before);

        assert!(handled);
        assert_eq!(state.cursor, edtui::Index2 { row: 0, col: 4 }, "the destination (top row) should land exactly on the aligned word, unmodified");
        let selection = state.selection.expect("should still have a selection");
        assert_eq!(selection.start, edtui::Index2 { row: 1, col: 3 }, "the anchor (bottom row, where the press started) should be trimmed back by one");
    }

    /// `Shift+Down` on the very last line makes no row progress at all
    /// -- must not panic (column-0 underflow) or otherwise touch
    /// anything.
    #[test]
    fn down_on_the_last_line_makes_no_adjustment() {
        let (mut state, cursor_before) = state_after_row_move("one\ntwo", 1, 1, crossterm::event::KeyCode::Down);
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Down, cursor_before);
        assert!(handled);
        assert_eq!(state.cursor, edtui::Index2 { row: 1, col: 1 }, "no row change was possible -- must not touch the column");
    }

    /// `Shift+Up` on the very first line makes no row progress at all --
    /// same guard, opposite direction.
    #[test]
    fn up_on_the_first_line_makes_no_adjustment() {
        let (mut state, cursor_before) = state_after_row_move("one\ntwo", 0, 1, crossterm::event::KeyCode::Up);
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Up, cursor_before);
        assert!(handled);
        assert_eq!(state.cursor, edtui::Index2 { row: 0, col: 1 });
        let selection = state.selection.expect("should still have a selection");
        assert_eq!(selection.start, cursor_before, "no row change was possible -- the anchor must not be trimmed");
    }
}
