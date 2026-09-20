use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    Frame,
};

use crate::theming::Theme;

/// The default F-key row, and the row shown while `Alt` is held
/// (`App::alt_held`). `F1`, `F2`, `F5`, `F7`, and `F8` actually change
/// binding (`Alt+F1`/`Alt+F2` open the left/right "change drive"
/// popup, `Alt+F5` opens Compare files, `Alt+F7` opens Find file,
/// `Alt+F8` opens History — all five are
/// `command_line/browsing/mod.rs`'s own raw-modifier special cases,
/// same reasoning as `Shift+F6`) — the rest keep their default action
/// and are just relabeled here to match, since Far Manager's real Alt
/// row doesn't rebind them either.
///
/// Laid out in 10 fixed-width columns spanning the full row width
/// (`Constraint::Ratio(1, 10)` each), rather than one flowing `Line`
/// with each label's own natural width — the column boundaries depend
/// only on `area`'s width, never on which label set is showing, so
/// switching between the default and `Alt` labels (`"Folder"` vs.
/// `"Find"`, six characters vs. four) can't shift anything to the
/// right of the changed column. An earlier version used a single
/// flowing line, which visibly reflowed every key from F7 onward the
/// instant `Alt` was pressed or released — reported as jarring; see
/// `tests::switching_alt_labels_does_not_shift_later_columns`, which
/// checks this against a real rendered buffer, not just the layout
/// math.
pub(super) fn draw_function_keys(frame: &mut Frame, area: Rect, theme: &Theme, alt: bool) {
    let labels = if alt { &ALT_LABELS } else { &DEFAULT_LABELS };

    for (column, &(key, label)) in function_key_columns(area).into_iter().zip(labels.iter()) {
        let line = Line::from(vec![
            Span::styled(format!("{key} "), Style::default().fg(theme.accent)),
            Span::styled(label, Style::default().fg(theme.text_dim)),
        ]);
        frame.render_widget(line, column);
    }
}

/// Keep every label at 6 characters or fewer — Far Manager's own
/// convention for this row (`"UserMn"`, `"MkFold"`, `"ConfMn"`, ...),
/// and not just cosmetic: with 10 equal-width columns spanning the
/// terminal, a longer label eats into (or overruns, at narrow widths)
/// the next column's space, since there's no gap reserved between
/// columns — found by hand as `"Bookmarks"` (9 characters) visibly
/// running into `"F3 View"` with no space between them. Enforced by
/// `tests::labels_stay_within_the_six_character_budget`, not just left
/// as a comment to remember.
pub(super) const DEFAULT_LABELS: [(&str, &str); 10] = [
    ("F1", "Help"), ("F2", "Menu"), ("F3", "View"), ("F4", "Edit"),
    ("F5", "Copy"), ("F6", "RenMov"), ("F7", "Folder"), ("F8", "Delete"),
    ("F9", "Menu"), ("F10", "Quit"),
];
pub(super) const ALT_LABELS: [(&str, &str); 10] = [
    ("F1", "DscLft"), ("F2", "DscRht"), ("F3", "View"), ("F4", "Edit"),
    ("F5", "Compar"), ("F6", "RenMov"), ("F7", "Find"), ("F8", "Histry"),
    ("F9", "Menu"), ("F10", "Quit"),
];

/// The 10 equal-width column rects the F-key row is split into —
/// depends only on `area`, never on the labels drawn inside it.
pub(super) fn function_key_columns(area: Rect) -> [Rect; 10] {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 10); 10])
        .split(area);
    std::array::from_fn(|i| columns[i])
}
