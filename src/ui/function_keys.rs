use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    Frame,
};

use crate::theming::Theme;

/// The F-key row, default or with `Alt` held. `Alt+F1/F2/F5/F7/F8` really
/// rebind (drive popups, Compare, Find file, History); the rest are only
/// relabeled, as in Far. Ten fixed-width columns, so switching label sets
/// can't shift later keys -- one flowing line reflowed everything from F7
/// on when `Alt` was pressed. History: docs/history/popups.md.
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

/// Labels stay within 6 characters (Far's convention): columns have no gap,
/// so "Bookmarks" ran into "F3 View". Enforced by
/// `labels_stay_within_the_six_character_budget`.
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
