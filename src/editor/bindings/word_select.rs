use edtui::actions::{Execute, MoveBackward, MoveWordBackward, MoveWordForward, MoveWordForwardToEndOfWord, SwitchMode};
use edtui::{EditorMode, EditorState, Index2, Lines};

/// `Ctrl+Shift+Left`/`Right` -- word-wise selection, VS Code-style:
/// forward lands on a word's last character, backward on its first,
/// and the selection covers only what the cursor actually travelled
/// over. `retracing` (from `Editor::word_select_touch`) says whether this
/// press gives back territory built in the other direction.
///
/// Keeps `state.cursor` exactly on the selection's live end -- `edtui`
/// relies on that everywhere. Landing on the anchor closes the
/// selection: `edtui`'s inclusive model can't show an empty one.
///
/// History (17 real reports, most obvious fixes already tried and
/// reverted): docs/history/word-select.md.
pub(in crate::editor) fn extend_word_selection(state: &mut EditorState, forward: bool, retracing: bool, true_anchor: &mut Option<Index2>) {
    let cursor_before = state.cursor;
    let selection_before = state.selection.as_ref().map(|s| (s.start, s.end));
    let starting_fresh_selection = state.mode != EditorMode::Visual;

    if starting_fresh_selection {
        SwitchMode(EditorMode::Visual).execute(state);
    }

    if forward {
        if retracing {
            retreat_forward_through_a_backward_walk(state, true_anchor);
        } else {
            if !advance_over_a_non_ascii_run(state, cursor_before) {
                MoveWordForwardToEndOfWord(1).execute(state);
            }
            if starting_fresh_selection && state.cursor == cursor_before {
                // Nowhere further right to go -- close rather than leave a
                // phantom one-cell selection on the anchor.
                SwitchMode(EditorMode::Normal).execute(state);
                SwitchMode(EditorMode::Insert).execute(state);
            }
        }
    } else {
        if starting_fresh_selection {
            *true_anchor = Some(cursor_before);
        }
        MoveWordBackward(1).execute(state);
        trim_anchor_off_a_word_it_never_visited(state, cursor_before, starting_fresh_selection);

        let landed_exactly_on_the_anchor = state.selection.as_ref().is_some_and(|s| s.start == state.cursor);
        if landed_exactly_on_the_anchor {
            // Everything has been given back -- close rather than show a
            // one-cell phantom selection (history: attempt 15).
            SwitchMode(EditorMode::Normal).execute(state);
            SwitchMode(EditorMode::Insert).execute(state);
        } else if retracing {
            retract_onto_the_separator(state);
        }
    }

    let selection_after = state.selection.as_ref().map(|s| (s.start, s.end));
    tracing::debug!(
        forward,
        ?cursor_before,
        ?selection_before,
        cursor_after = ?state.cursor,
        ?selection_after,
        "extend_word_selection"
    );
}

/// A fresh backward selection never includes the cell it started on:
/// trims the anchor one column back whenever a real character sits
/// there -- the same rule character-wise `Shift+Left` follows
/// (`shift_select.rs::backward_anchor`). History: attempts 7, 12, 13.
fn trim_anchor_off_a_word_it_never_visited(state: &mut EditorState, cursor_before: Index2, fresh_selection: bool) {
    if !fresh_selection || cursor_before.col == 0 {
        return;
    }

    if state.lines.get(cursor_before).is_none() {
        return;
    }

    if let Some(selection) = state.selection.as_mut() {
        if selection.start == cursor_before {
            selection.start.col -= 1;
        }
    }
}

/// Retracting a forward-built selection: after `MoveWordBackward`
/// landed on the retracted word's own first character, steps onto the
/// separator right before it (whatever character that is -- a word
/// motion always stops at a class boundary). *Whether* to retract at all
/// is the caller's `retracing`, never guessed here. History: attempts 8, 9.
fn retract_onto_the_separator(state: &mut EditorState) {
    if state.cursor.col == 0 {
        return;
    }

    let left_of_landing = Index2 { row: state.cursor.row, col: state.cursor.col - 1 };
    if state.lines.get(left_of_landing).is_some() {
        MoveBackward(1).execute(state);
    }
}

/// `Right` against a selection built by walking backward: retraces that
/// walk with plain `MoveWordForward` (the exact mirror of
/// `MoveWordBackward`), and closes once it reaches *or passes* the anchor
/// (the trimmed anchor is never on `MoveWordForward`'s own grid), putting
/// the cursor back on the true, untrimmed start (`true_anchor`).
/// History: attempts 14, 16.
fn retreat_forward_through_a_backward_walk(state: &mut EditorState, true_anchor: &mut Option<Index2>) {
    MoveWordForward(1).execute(state);

    let Some(anchor) = state.selection.as_ref().map(|s| s.start) else {
        return;
    };
    let reached_or_passed_the_anchor = (state.cursor.row, state.cursor.col) >= (anchor.row, anchor.col);
    if !reached_or_passed_the_anchor {
        return;
    }

    let restore_to = true_anchor.take().unwrap_or(anchor);
    state.cursor = restore_to;
    if let Some(selection) = state.selection.as_mut() {
        selection.end = restore_to;
    }
    SwitchMode(EditorMode::Normal).execute(state);
    SwitchMode(EditorMode::Insert).execute(state);
}


/// Forward over a non-ASCII run (em dash, Cyrillic, ...), which
/// `MoveWordForwardToEndOfWord` never moves over (`edtui`'s
/// `CharacterClass::Unknown` never equals itself). Returns `false`
/// without touching anything for an ordinary ASCII run, so the caller
/// falls back to the `edtui` action. History: attempt 17.
fn advance_over_a_non_ascii_run(state: &mut EditorState, cursor_before: Index2) -> bool {
    // Same lead-in `MoveWordForwardToEndOfWord` performs before it
    // starts scanning: step onto the next cell (or the next line, at
    // the end of this one -- `false` here, same as declining, since
    // there's nothing left to advance over at all).
    let lead_in = if state.lines.is_last_col(cursor_before) {
        if state.lines.is_last_row(cursor_before) {
            return false;
        }
        Index2::new(cursor_before.row + 1, 0)
    } else {
        Index2::new(cursor_before.row, cursor_before.col + 1)
    };
    let start = skip_to_word_scan_start(&state.lines, lead_in);

    let Some(&first_char) = state.lines.get(start) else {
        return false; // nothing left in the buffer at all
    };
    if is_known_character_class(first_char) {
        return false; // an ordinary ASCII run -- edtui's own action already handles this
    }

    // Walk forward while this run stays non-ASCII, the same "advance
    // while still the same class" shape `MoveWordForwardToEndOfWord`
    // itself uses -- including its own same "stop at a row's last
    // column" boundary, so this never silently crosses a line the real
    // action wouldn't have either.
    let mut landing = start;
    for (next_char, index) in state.lines.iter().from(start) {
        let Some(&c) = next_char else { break };
        if is_known_character_class(c) {
            break;
        }
        landing = index;
        if state.lines.is_last_col(index) {
            break;
        }
    }

    state.cursor = landing;
    if let Some(selection) = state.selection.as_mut() {
        selection.end = landing;
    }
    true
}

/// Mirrors `edtui`'s own (`pub(crate)`) lead-in for its forward word
/// motions: skip fully empty rows, then ASCII whitespace within the row
/// it lands on -- without crossing into a further row, as `edtui` doesn't.
fn skip_to_word_scan_start(lines: &Lines, mut index: Index2) -> Index2 {
    while lines.is_empty_row(index.row) == Some(true) {
        index = Index2::new(index.row + 1, 0);
    }
    while lines.get(index).is_some_and(char::is_ascii_whitespace) {
        index.col += 1;
    }
    index
}

/// Whether `c` falls in one of `edtui`'s own `CharacterClass`es --
/// anything else is `Unknown`, which its forward word motion is stuck on.
fn is_known_character_class(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c.is_ascii_punctuation() || c.is_ascii_whitespace()
}


#[cfg(test)]
mod tests;
#[cfg(test)]
mod word_selection_on_realistic_text;
