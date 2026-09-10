use ratatui::{backend::TestBackend, layout::Rect, Terminal};

use super::*;

/// Reads back the visible text of row `y`, column start inclusive,
/// by concatenating each cell's symbol — the same shape a real
/// terminal would show, so an `x` found in this string lines up
/// with an actual on-screen column.
fn rendered_row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn render_function_keys(width: u16, alt: bool) -> String {
    let backend = TestBackend::new(width, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal
        .draw(|frame| draw_function_keys(frame, Rect::new(0, 0, width, 1), &theme, alt))
        .unwrap();
    rendered_row(&terminal, 0)
}

/// The actual reported bug: the row used to be one flowing `Line`
/// with each label's own natural width, so switching `Alt` (e.g.
/// `"Folder"` <-> `"Find"`, six characters vs. four) visibly
/// reflowed every key from F7 onward. Column layout fixes this —
/// this test renders both label sets into a real buffer and checks
/// that every later key's own column (found by its own `"F<n> "`
/// prefix) starts at the exact same `x` regardless of which set is
/// showing, not just that the layout math looks right on paper.
#[test]
fn switching_alt_labels_does_not_shift_later_columns() {
    let default_row = render_function_keys(120, false);
    let alt_row = render_function_keys(120, true);

    for key in ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"] {
        let default_pos = default_row.find(key).unwrap_or_else(|| panic!("{key} missing from default row: {default_row:?}"));
        let alt_pos = alt_row.find(key).unwrap_or_else(|| panic!("{key} missing from alt row: {alt_row:?}"));
        assert_eq!(default_pos, alt_pos, "{key} shifted position when switching labels (default row: {default_row:?}, alt row: {alt_row:?})");
    }
}

/// The row should use the full width available to it (10 equal
/// columns spanning `area`), not just as much as the longest
/// label set happens to need — `function_key_columns`'s last
/// column should reach the right edge.
#[test]
fn columns_span_the_full_available_width() {
    let columns = function_key_columns(Rect::new(0, 0, 100, 1));
    let last = columns.last().unwrap();
    assert_eq!(last.x + last.width, 100, "last column should reach the right edge: {columns:?}");
}

/// Regression test for a real reported bug: `"Bookmarks"` (9
/// characters) visibly ran into the next column with no gap, since
/// equal-width columns reserve no padding between them. Caps every
/// label (both label sets) at Far Manager's own 6-character
/// convention (`"UserMn"`, `"MkFold"`, `"ConfMn"`, ...) so this
/// can't silently regress if a label is ever lengthened.
#[test]
fn labels_stay_within_the_six_character_budget() {
    for &(key, label) in DEFAULT_LABELS.iter().chain(ALT_LABELS.iter()) {
        assert!(label.len() <= 6, "{key}'s label {label:?} is {} characters, over the 6-character budget", label.len());
    }
}
