use std::sync::atomic::Ordering;

use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::explorer::{FindFileField, FindFilePhase, FindFileState};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders whichever phase is current. Returns where the real terminal
/// cursor should sit — only meaningful during `Typing` (same mechanism
/// as the command line's own cursor, `ui::draw`); `None` during
/// `Searching`/`Results`, neither of which has a text entry to place a
/// cursor in.
pub fn draw_find_file(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) -> Option<Position> {
    match state.phase {
        FindFilePhase::Typing => Some(draw_typing(frame, area, state, theme, style)),
        FindFilePhase::Searching => {
            draw_searching(frame, area, state, theme, style);
            None
        }
        FindFilePhase::Results => {
            draw_results(frame, area, state, theme, style);
            None
        }
    }
}

/// "1 result" / "N results" / "N+ results" -- the plural-agreement half
/// of the results title's own count-and-timing summary. `capped`
/// (`FindFileState::results_capped`) appends a "+", the same widely
/// understood "there's more, this isn't the real total" convention a
/// lot of search/notification UIs already use -- added directly after
/// a report that a search hitting exactly `find_file_max_results` (200,
/// the default) looked like a complete, successful search with no way
/// to tell it wasn't, compared side by side against real Far Manager
/// finding well over double that on the same tree.
fn result_count_label(count: usize, capped: bool) -> String {
    let plus = if capped { "+" } else { "" };
    if count == 1 && !capped {
        "1 result".to_string()
    } else {
        format!("{count}{plus} results")
    }
}

/// Sub-second durations render as whole milliseconds (a decimal second
/// value would show mostly zeroes for the common, fast case); a full
/// second or more switches to seconds with two decimal places, which
/// is roughly the precision a human glancing at a search's own timing
/// actually cares about either way.
fn format_duration(duration: std::time::Duration) -> String {
    if duration.as_secs() >= 1 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn draw_typing(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) -> Position {
    // `Rounded` gets a separator right under the title, matching
    // `ui/theme_menu.rs`'s own color-scheme picker (the reference look
    // requested directly) -- `Classic` stays as plain as every other
    // Classic-style popup, no separator of its own.
    let has_separator = style == PopupStyle::Rounded;
    // 5 content rows (name label, name value, content label, content
    // value, hint) -- one label+value pair per field, matching Far
    // Manager's own two-field Find file dialog (name mask, and a
    // separate "Text to find" for searching inside files) -- plus the
    // separator's own row when there is one, plus 2 for a plain border.
    // `Rounded`'s own extra padding/title chrome needs more room for the
    // same rows, or they'd get clipped to nothing
    // (`popup::chrome_extra_rows`'s own doc comment has the exact
    // accounting).
    let height = 7 + popup::chrome_extra_rows(style) + u16::from(has_separator);
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

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    if has_separator {
        constraints.insert(0, Constraint::Length(1));
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separator);
    if has_separator {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
    }

    // The active field's own label is bold/accented so it's clear which
    // field `Tab` and typed characters currently reach -- the inactive
    // one stays plain, same visual weight every other unfocused label in
    // this app already gets.
    let label_style = |field: FindFileField| {
        if state.active_field == field {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        }
    };

    let name_label = Line::from(Span::styled("File name to find:", label_style(FindFileField::Name)));
    frame.render_widget(name_label, rows[content_start]);

    let name_row = rows[content_start + 1];
    let name_value = Line::from(Span::styled(state.query.clone(), Style::default().fg(theme.text)));
    frame.render_widget(name_value, name_row);

    let content_label = Line::from(Span::styled("Text to find:", label_style(FindFileField::Content)));
    frame.render_widget(content_label, rows[content_start + 2]);

    let content_row = rows[content_start + 3];
    let content_value = Line::from(Span::styled(state.content_query.clone(), Style::default().fg(theme.text)));
    frame.render_widget(content_value, content_row);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" search  ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" switch field  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[content_start + 4]);

    let (cursor_row, cursor) = match state.active_field {
        FindFileField::Name => (name_row, state.cursor),
        FindFileField::Content => (content_row, state.content_cursor),
    };
    Position { x: cursor_row.x + cursor as u16, y: cursor_row.y }
}

/// `FindFilePhase::Searching`: a live "please wait" screen shown while
/// the background search (`background.rs`) is still running --
/// requested directly for a comparison against real Far Manager's own
/// dialog, which shows the same kind of thing rather than blocking with
/// nothing on screen. Reads `state.pending`'s own live counters
/// directly (`SearchProgress`, `Relaxed` loads -- this is just a status
/// line refreshed every redraw, not something anything synchronizes
/// real work on) rather than needing its own copy of the numbers.
fn draw_searching(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) {
    let has_separator = style == PopupStyle::Rounded;
    // 2 content rows (status line, hint) -- deliberately terser than
    // `Typing`'s 5, since there's nothing to edit here.
    let height = 4 + popup::chrome_extra_rows(style) + u16::from(has_separator);
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Find file ")),
        PopupStyle::Rounded => Line::from(Span::styled("Find file", Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, 50, height);

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1)];
    if has_separator {
        constraints.insert(0, Constraint::Length(1));
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separator);
    if has_separator {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
    }

    // `state.pending` should always be `Some` here -- `Searching` is
    // only ever entered alongside setting it (`input.rs::run_search`)
    // -- but this degrades to a plain "Searching..." with zero counts
    // rather than panicking if that invariant were ever somehow broken.
    let status_text = match &state.pending {
        Some(pending) => {
            let visited = pending.progress.visited.load(Ordering::Relaxed);
            let found = pending.progress.found.load(Ordering::Relaxed);
            format!("Searching... {visited} visited, {found} found ({})", format_duration(pending.started.elapsed()))
        }
        None => "Searching...".to_string(),
    };
    let status = Line::from(Span::styled(status_text, Style::default().fg(theme.text)));
    frame.render_widget(status, rows[content_start]);

    let hint = Line::from(vec![
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[content_start + 1]);
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
    // A content query, when set, is folded into the title too -- Far
    // Manager's own results view names both the mask and the searched
    // text once a search runs.
    let base_title = if state.content_query.is_empty() {
        format!("Find file: \"{}\"", state.query)
    } else {
        format!("Find file: \"{}\" containing \"{}\"", state.query, state.content_query)
    };
    // The result count and how long the search actually took -- Far
    // Manager's own dialog shows both once a search finishes, requested
    // directly here too. `search_duration` is `None` only if this popup
    // somehow reached `Results` without ever calling `run_search`
    // (doesn't happen in practice, but the title degrades gracefully
    // rather than showing a fabricated "0ms" if it ever did).
    let title_text = match state.search_duration {
        Some(duration) => format!("{base_title} — {} in {}", result_count_label(state.results.len(), state.results_capped), format_duration(duration)),
        None => base_title,
    };
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(format!(" {title_text} "))),
        PopupStyle::Rounded => Line::from(Span::styled(title_text, Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
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
            content_query: String::new(),
            content_cursor: 0,
            active_field: FindFileField::Name,
            name_history_index: None,
            content_history_index: None,
            results: (0..count).map(|i| PathBuf::from(format!("C:/dev/file_{i:04}.txt"))).collect(),
            selected,
            pending: None,
            search_duration: Some(std::time::Duration::from_millis(5)),
            results_capped: false,
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
        let state = FindFileState { phase: FindFilePhase::Typing, query: String::new(), cursor: 0, content_query: String::new(), content_cursor: 0, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, pending: None, search_duration: None, results_capped: false, export_message: None };
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
        let state = FindFileState { phase: FindFilePhase::Typing, query: "abc".to_string(), cursor: 3, content_query: String::new(), content_cursor: 0, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, pending: None, search_duration: None, results_capped: false, export_message: None };
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

    /// Regression coverage for the Far-analogous "Text to find" field:
    /// both labels and both typed values should be visible while typing,
    /// and the cursor should follow whichever field is currently active.
    #[test]
    fn typing_phase_shows_both_fields_and_the_cursor_follows_the_active_one() {
        let mut state = FindFileState { phase: FindFilePhase::Typing, query: "read".to_string(), cursor: 4, content_query: "todo".to_string(), content_cursor: 4, active_field: FindFileField::Content, name_history_index: None, content_history_index: None, results: vec![], selected: 0, pending: None, search_duration: None, results_capped: false, export_message: None };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut cursor = None;
        terminal.draw(|frame| {
            cursor = draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
        }).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("File name to find"), "name label should be visible: {text}");
        assert!(text.contains("Text to find"), "content label should be visible: {text}");
        assert!(text.contains("read"), "typed name query should be visible: {text}");
        assert!(text.contains("todo"), "typed content query should be visible: {text}");
        assert!(text.contains("switch field"), "hint row should mention Tab: {text}");

        let content_label_y = text.lines().position(|line| line.contains("Text to find")).unwrap() as u16;
        let cursor = cursor.expect("Typing phase should place a cursor");
        assert_eq!(cursor.y, content_label_y + 1, "cursor should sit on the content field's own value row, not the name field's");

        state.active_field = FindFileField::Name;
        let mut cursor_on_name = None;
        terminal.draw(|frame| {
            cursor_on_name = draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
        }).unwrap();
        let name_label_y = {
            let buffer = terminal.backend().buffer();
            let text: String = (0..buffer.area.height)
                .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            text.lines().position(|line| line.contains("File name to find")).unwrap() as u16
        };
        assert_eq!(cursor_on_name.unwrap().y, name_label_y + 1, "cursor should follow active_field back to the name row");
    }

    /// A non-empty content query should show up in the results title
    /// too, alongside the name query -- both are part of what the
    /// search actually ran with.
    #[test]
    fn results_title_includes_the_content_query_when_set() {
        let mut state = state_with(1, 0);
        state.content_query = "needle".to_string();
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
        assert!(text.contains("needle"), "the content query should show up in the title: {text}");
    }

    /// Regression coverage for the real request: while a search is
    /// still running (`FindFilePhase::Searching`), the popup should
    /// show live progress and an `Esc`-to-cancel hint -- Far Manager's
    /// own dialog shows both instead of blocking with nothing to look
    /// at. Uses a real background search (`explorer::spawn_search`,
    /// re-exported test-only) against a scratch directory with enough
    /// files that it's very unlikely to have already finished by the
    /// time this reads the popup's own text.
    #[test]
    fn searching_phase_shows_live_progress_and_a_cancel_hint() {
        let dir = crate::test_support::unique_scratch_dir("find-file-ui-searching");
        for i in 0..500 {
            std::fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
        }
        let pending = crate::explorer::spawn_search(dir, "file".to_string(), String::new());
        let state = FindFileState { phase: FindFilePhase::Searching, query: "file".to_string(), cursor: 4, content_query: String::new(), content_cursor: 0, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, pending: Some(pending), search_duration: None, results_capped: false, export_message: None };
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
        assert!(text.contains("Searching"), "should show a searching status: {text}");
        assert!(text.contains("visited"), "should show live progress: {text}");
        assert!(text.contains("cancel"), "should hint that Esc cancels: {text}");
    }

    /// `state.pending` should always be `Some` while `Searching`, but
    /// `draw_searching` shouldn't panic even if that invariant were
    /// ever somehow broken -- degrades to a plain status line instead.
    #[test]
    fn searching_phase_does_not_panic_with_no_pending_search() {
        let state = FindFileState { phase: FindFilePhase::Searching, query: String::new(), cursor: 0, content_query: String::new(), content_cursor: 0, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, pending: None, search_duration: None, results_capped: false, export_message: None };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
        }).unwrap();
    }

    /// Regression coverage for the real request: the results popup
    /// should show how long the search actually took, alongside the
    /// result count -- Far Manager's own dialog shows both once a
    /// search finishes.
    #[test]
    fn results_title_shows_the_result_count_and_search_duration() {
        let mut state = state_with(3, 0);
        state.search_duration = Some(std::time::Duration::from_millis(42));
        let text = rendered(&state);
        assert!(text.contains("3 results"), "should show the result count: {text}");
        assert!(text.contains("42ms"), "should show the elapsed time: {text}");
    }

    /// Regression coverage for the real report: a search that hit
    /// exactly `find_file_max_results` used to render as a plain,
    /// precise-looking count with no way to tell it wasn't the real
    /// total -- `results_capped` should turn that into a "+" instead.
    #[test]
    fn results_title_shows_a_plus_when_results_are_capped() {
        let mut state = state_with(200, 0);
        state.results_capped = true;
        let text = rendered(&state);
        assert!(text.contains("200+ results"), "a capped result count should show a +: {text}");
    }

    #[test]
    fn results_title_shows_singular_result_and_seconds_over_a_full_second() {
        let mut state = state_with(1, 0);
        state.search_duration = Some(std::time::Duration::from_millis(1500));
        let text = rendered(&state);
        assert!(text.contains("1 result "), "should use the singular form for exactly one result: {text}");
        assert!(text.contains("1.50s"), "should switch to seconds past the one-second mark: {text}");
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
