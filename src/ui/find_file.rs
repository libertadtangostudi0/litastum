use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::explorer::{FindFilePhase, FindFileState};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders whichever phase is current. Returns where the real terminal
/// cursor should sit — only meaningful during `Typing` (same mechanism
/// as the command line's own cursor, `ui::draw`); `None` during
/// `Results`, which has no text entry to place a cursor in.
pub fn draw_find_file(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) -> Option<Position> {
    match state.phase {
        FindFilePhase::Typing => Some(draw_typing(frame, area, state, theme, style)),
        FindFilePhase::Results => {
            draw_results(frame, area, state, theme, style);
            None
        }
    }
}

fn draw_typing(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) -> Position {
    // `Rounded` gets a separator right under the title, matching
    // `ui/theme_menu.rs`'s own color-scheme picker (the reference look
    // requested directly) -- `Classic` stays as plain as every other
    // Classic-style popup, no separator of its own.
    let has_separator = style == PopupStyle::Rounded;
    // 3 content rows (label, query, hint), plus the separator's own row
    // when there is one, plus 2 for a plain border -- `Rounded`'s own
    // extra padding/title chrome needs more room for the same rows, or
    // they'd get clipped to nothing (`popup::chrome_extra_rows`'s own
    // doc comment has the exact accounting).
    let height = 5 + popup::chrome_extra_rows(style) + u16::from(has_separator);
    // `Classic` bakes " Find file " (with its own margin spaces) into
    // the border; `Rounded` already gets real margin from `draw_frame`'s
    // own padding, so an un-padded bold title -- matching `ui/theme_menu
    // .rs`'s "Color scheme" exactly -- avoids stacking that margin twice
    // and sitting one column further right than every other Rounded
    // popup's title.
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Find file ")),
        PopupStyle::Rounded => Line::from(Span::styled("Find file", Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, 50, height);

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    if has_separator {
        constraints.insert(0, Constraint::Length(1));
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separator);
    if has_separator {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
    }

    let label = Line::from(Span::styled("File name to find:", Style::default().fg(theme.text)));
    frame.render_widget(label, rows[content_start]);

    let query_row = rows[content_start + 1];
    let query = Line::from(Span::styled(state.query.clone(), Style::default().fg(theme.text)));
    frame.render_widget(query, query_row);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" search  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[content_start + 2]);

    Position { x: query_row.x + state.cursor as u16, y: query_row.y }
}

/// Fixed popup height, independent of `state.results.len()` -- now that
/// the list actually scrolls (see the `ListState` below), there's no
/// reason for the popup itself to keep growing with the result count
/// the way it used to. Picked to match `ui/theme_menu.rs`'s own
/// color-scheme popup height directly, per an explicit side-by-side
/// comparison request -- that popup's own formula (`themes.len() + 13`)
/// comes out to `21` for the 8 themes bundled with this repo, so this
/// reuses that same number rather than inventing an unrelated one.
const RESULTS_HEIGHT: u16 = 21;

fn draw_results(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) {
    // `Rounded` gets a bold title with a separator right under it
    // (matching `ui/theme_menu.rs`'s own color-scheme picker, the
    // reference requested directly) and another separator right before
    // the footer hints (matching `ui/confirm.rs`'s delete popup) --
    // `Classic` stays plain, title baked into the border, no separators
    // of its own, same as every other Classic-style popup.
    let has_separators = style == PopupStyle::Rounded;
    let height = (RESULTS_HEIGHT + 2 * u16::from(has_separators)).min(area.height);
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(format!(" Find file: \"{}\" ", state.query))),
        PopupStyle::Rounded => Line::from(Span::styled(format!("Find file: \"{}\"", state.query), Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, 70, height);

    // Two fixed rows for the export message (label + detail, e.g.
    // "Exported to:" / the actual path) even when there isn't one --
    // simpler than resizing the popup depending on whether a message
    // is currently showing, at the cost of a little blank space in the
    // common "haven't exported yet" case.
    let mut constraints = vec![Constraint::Min(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    if has_separators {
        constraints.insert(0, Constraint::Length(1)); // separator under the title
        constraints.insert(4, Constraint::Length(1)); // separator before the hints
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separators);
    let hint_row_index = if has_separators {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
        frame.render_widget(popup::separator(inner.width, theme), rows[4]);
        5
    } else {
        3
    };

    if state.results.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled("No matches found", Style::default().fg(theme.text_dim))));
        frame.render_widget(empty, rows[content_start]);
    } else {
        let items: Vec<ListItem> = state
            .results
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let style = if index == state.selected {
                    popup::selected_row_style(theme)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(path.to_string_lossy().into_owned(), style)))
            })
            .collect();
        // A result count in the thousands (a big tree, a broad query)
        // used to render every item straight into this fixed-height
        // area with no scroll offset at all -- reported directly, the
        // same "List with no ListState doesn't auto-scroll" gap
        // `Panel`'s own entry grid hit earlier (see its own doc comment
        // in `explorer/panel/mod.rs`), and already fixed once in this
        // codebase for `ui/theme_menu.rs`'s picker the same way: a real
        // `ListState` tracking the selected index, which `List` then
        // scrolls to keep in view on its own.
        let list = List::new(items);
        let mut list_state = ListState::default().with_selected(Some(state.selected));
        frame.render_stateful_widget(list, rows[content_start], &mut list_state);
    }

    if let Some((label, detail)) = &state.export_message {
        let label_line = Line::from(Span::styled(label.clone(), Style::default().fg(theme.text_dim)));
        frame.render_widget(label_line, rows[content_start + 1]);
        // A real Downloads path is easily wide enough to overflow the
        // popup on one line together with the label -- ratatui clips
        // rather than wraps a `Line` that's too long for its area, so
        // splitting the detail onto its own row (still just clipped if
        // it's *itself* wider than the popup, but that's a much rarer
        // case than "label + path together" was) is the fix here, not
        // a text-wrapping widget for what's meant to be a one-line
        // status.
        let detail_line = Line::from(Span::styled(detail.clone(), Style::default().fg(theme.text)));
        frame.render_widget(detail_line, rows[content_start + 2]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" go to  ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" peek  ", Style::default().fg(theme.text_dim)),
        Span::styled("F4", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" edit  ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+S", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" export  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[hint_row_index]);
}


#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn state_with(count: usize, selected: usize) -> FindFileState {
        FindFileState {
            phase: FindFilePhase::Results,
            query: "x".to_string(),
            cursor: 0,
            results: (0..count).map(|i| PathBuf::from(format!("C:/dev/file_{i:04}.txt"))).collect(),
            selected,
            export_message: None,
        }
    }

    fn rendered(state: &FindFileState) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_find_file(frame, frame.area(), state, &theme, PopupStyle::Rounded);
        }).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Regression test for the real report: a result count far past
    /// what the (terminal-height-clamped) popup can show at once used
    /// to render every item into a fixed-height list with no scroll
    /// offset at all -- the selected row, deep into a list of
    /// thousands, was simply invisible, scrolled off past the bottom of
    /// the rendered area with nothing to bring it into view. A real
    /// `ListState` (see the call site's own doc comment) should keep
    /// whichever row is selected actually on screen no matter how far
    /// into a long list it is.
    #[test]
    fn selecting_a_result_far_down_a_long_list_scrolls_it_into_view() {
        let state = state_with(2000, 1500);

        let text = rendered(&state);

        assert!(text.contains("file_1500"), "the selected result should be scrolled into view:\n{text}");
    }

    #[test]
    fn a_short_result_list_needs_no_scrolling_to_show_the_selection() {
        let state = state_with(3, 2);

        let text = rendered(&state);

        assert!(text.contains("file_0002"));
    }

    /// `Rounded` gets a separator under the title (matching
    /// `ui/theme_menu.rs`'s own color-scheme picker) and another right
    /// before the footer hints (matching `ui/confirm.rs`'s delete
    /// popup) -- requested directly, alongside a screenshot of the
    /// color-scheme picker's own title-plus-line look.
    /// Requested directly, alongside the color-scheme picker's own
    /// title-plus-line screenshot: the title should also get a bold,
    /// un-padded look and a separator right under it in the Results
    /// phase, same as `Typing` already has.
    #[test]
    fn rounded_style_results_show_a_separator_under_the_title() {
        let state = state_with(2, 0);
        let text = rendered(&state);
        let title_line_index = text.lines().position(|line| line.contains("Find file")).expect("title should render");
        let line_below = text.lines().nth(title_line_index + 1).unwrap();
        assert!(line_below.contains('─'), "a separator should sit right below the title: {line_below:?}");
    }

    #[test]
    fn rounded_style_results_show_a_separator_before_the_hints() {
        let state = state_with(2, 0);
        let text = rendered(&state);
        let hint_line_index = text.lines().position(|line| line.contains("go to")).expect("hint row should render");
        let line_above = text.lines().nth(hint_line_index - 1).unwrap();
        assert!(line_above.contains('─'), "a separator should sit right above the footer hints: {line_above:?}");
    }

    #[test]
    fn rounded_style_typing_shows_a_separator_under_the_title() {
        let state = FindFileState { phase: FindFilePhase::Typing, query: String::new(), cursor: 0, results: vec![], selected: 0, export_message: None };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
        }).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let title_line_index = text.lines().position(|line| line.contains("Find file")).expect("title should render");
        let line_below = text.lines().nth(title_line_index + 1).unwrap();
        assert!(line_below.contains('─'), "a separator should sit right below the title: {line_below:?}");
    }

    /// Regression coverage for a real report: the `Typing` phase's
    /// popup used a fixed height (5) sized for `Classic`'s tighter
    /// chrome (just a 2-row border) -- under `Rounded`, whose border +
    /// padding + title row eat 7 rows on their own, that left zero room
    /// for the label/query/hint content, and the popup rendered
    /// entirely blank. `draw_typing` now grows the popup by
    /// `popup::chrome_extra_rows(style)` to keep the same 3 content rows
    /// visible under either style.
    #[test]
    fn rounded_style_typing_phase_shows_the_label_and_hint_not_just_an_empty_box() {
        let state = FindFileState { phase: FindFilePhase::Typing, query: "abc".to_string(), cursor: 3, results: vec![], selected: 0, export_message: None };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
        }).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("File name to find"), "label should be visible: {text}");
        assert!(text.contains("abc"), "typed query should be visible: {text}");
        assert!(text.contains("search"), "hint row should be visible: {text}");
    }

    /// Both `PopupStyle`s should render the same query/results content
    /// -- only the chrome (border/padding/title placement) differs.
    #[test]
    fn classic_style_still_shows_the_query_and_results() {
        let state = state_with(3, 0);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Classic);
        }).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("file_0000"));
        assert!(text.contains("Find file"));
    }
}
