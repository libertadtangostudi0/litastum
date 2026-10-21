use std::path::Path;

use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::command_line::{matching_history, CommandHistoryMenu, Suggestion};
use crate::text_field::TextField;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// The always-live command line: `"{cwd}> {typed}"`, like Far's (the
/// path, not a bare `>`). Returns where the cursor goes. The shell-profile hint on the right was removed as clutter;
/// `Ctrl+P`'s picker shows it.
pub(super) fn draw_command_line(frame: &mut Frame, area: Rect, cwd: &Path, command_line: &TextField, theme: &Theme) -> Position {
    let prefix = shortened_prefix(&format!("{}> ", cwd.display()), area.width / 2);
    let prefix_width = (prefix.chars().count() as u16).min(area.width);
    frame.render_widget(Line::from(Span::styled(prefix, Style::default().fg(theme.command_line_prefix).add_modifier(Modifier::BOLD))), area);

    // Reported: a line longer than the row hid its end, the caret past
    // the edge. The text scrolls sideways to keep the caret in view, as
    // the editor's search box does.
    let text_area = Rect { x: area.x + prefix_width, width: area.width - prefix_width, ..area };
    super::text_field::draw_field(frame, text_area, command_line, theme)
}


/// `prefix` as is, or its end after a `…` when it's wider than `max`
/// cells -- a deep directory left the typed line no room.
fn shortened_prefix(prefix: &str, max: u16) -> String {
    let count = prefix.chars().count();
    let max = usize::from(max).max(4);
    if count <= max {
        return prefix.to_string();
    }
    let tail: String = prefix.chars().skip(count - (max - 1)).collect();
    format!("…{tail}")
}

/// Fixed height (the list scrolls), the same 21 as the Find file results;
/// 70 columns wide, since long commands were clipped at 60. History: docs/history/popups.md.
const HISTORY_HEIGHT: u16 = 21;
const HISTORY_WIDTH: u16 = 70;

/// The History popup: filtered live by the command line's text, newest
/// last, or a hint when nothing matches. Built on `popup::draw_frame`,
/// which it had missed, so `Rounded` did nothing here.
pub fn draw_command_history(frame: &mut Frame, area: Rect, menu: &CommandHistoryMenu, history: &[String], query: &str, theme: &Theme, style: PopupStyle) {
    let matches = matching_history(history, query);
    let height = (HISTORY_HEIGHT + popup::chrome_extra_rows(style)).min(area.height);
    // `Rounded` gets a separator right before the footer hint, matching
    // `ui/find_file.rs`'s own results popup and `ui/confirm.rs`'s
    // delete popup; `Classic` stays plain, no separator, same as every
    // other Classic-style popup.
    let has_separator = style == PopupStyle::Rounded;
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" History ")),
        PopupStyle::Rounded => Line::from(Span::styled("History", Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, HISTORY_WIDTH, height);

    let mut constraints = vec![Constraint::Min(1), Constraint::Length(1)];
    if has_separator {
        constraints.insert(1, Constraint::Length(1));
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let hint_row_index = if has_separator {
        frame.render_widget(popup::separator(inner.width, theme), rows[1]);
        2
    } else {
        1
    };

    if matches.is_empty() {
        let message = if history.is_empty() { "No commands run yet" } else { "No matching commands" };
        let empty = Paragraph::new(Line::from(Span::styled(message, Style::default().fg(theme.text_dim))));
        frame.render_widget(empty, rows[0]);
    } else {
        let items: Vec<ListItem> = matches
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let style = if index == menu.selected {
                    popup::selected_row_style(theme)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(entry.as_str().to_string(), style)))
            })
            .collect();
        // A `ListState` keeps the selected entry in view; without one `List`
        // doesn't scroll, and the selection disappeared below the border.
        let list = List::new(items);
        let mut list_state = ListState::default().with_selected(Some(menu.selected));
        frame.render_stateful_widget(list, rows[0], &mut list_state);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" run  ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" edit  ", Style::default().fg(theme.text_dim)),
        Span::styled("F8", Style::default().fg(theme.danger).add_modifier(Modifier::BOLD)),
        Span::styled(" delete  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[hint_row_index]);
}

/// The suggestions that pop up above the command line on a match, as in
/// Far (not a centered popup): history entries, then the active panel's
/// names (in `accent`). While shown, `Up`/`Down` move and `Tab` accepts
/// (`browsing.rs`); the list scrolls to keep the selection in view.
pub fn draw_suggestions(frame: &mut Frame, command_line_area: Rect, suggestions: &[Suggestion], selected: usize, theme: &Theme) {
    let height = (suggestions.len() as u16 + 2).min(10);
    let popup = Rect {
        x: command_line_area.x,
        y: command_line_area.y.saturating_sub(height),
        // Full width of the panels above (`command_line_area` already
        // spans that same width, same as `root[0]`/`root[1]` in
        // `ui::draw`) rather than clamped to some fixed max -- a long
        // command needs the room, and there's a full row's worth of
        // width sitting right there unused.
        width: command_line_area.width,
        height,
    };

    frame.render_widget(Clear, popup);

    let has_names = suggestions.iter().any(|suggestion| matches!(suggestion, Suggestion::File { .. }));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(if has_names { " Suggestions " } else { " History " });
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = usize::from(inner.height);
    let first = (selected + 1).saturating_sub(rows);
    let items: Vec<ListItem> = suggestions
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .map(|(index, suggestion)| {
            let style = if index == selected {
                popup::selected_row_style(theme)
            } else if matches!(suggestion, Suggestion::File { .. }) {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(suggestion.label(), style)))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_line::CommandHistoryMenu;
    use crate::theming::Theme;

    fn line_with(text: &str) -> TextField {
        let mut field = TextField::new();
        field.set_text(text);
        field
    }

    /// Draws the command line into a `width`-wide row: what shows, and the
    /// cursor's column.
    fn drawn_line(cwd: &str, field: &TextField, width: u16) -> (String, u16) {
        let mut cursor = Position::default();
        let text = crate::test_support::render_text(width, 1, |frame| cursor = draw_command_line(frame, frame.area(), Path::new(cwd), field, &Theme::dark()));
        (text.lines().next().unwrap_or("").to_string(), cursor.x)
    }

    /// Reported: a line longer than the row hid its end, and the caret
    /// went past the edge. The text scrolls to keep the caret in view.
    #[test]
    fn a_long_line_scrolls_to_keep_the_caret_in_view() {
        let typed: String = (0..80).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let (shown, cursor) = drawn_line("C:\\work", &line_with(&typed), 40);

        assert!(cursor < 40, "the caret stays on screen: {cursor}");
        assert!(shown.trim_end().ends_with(&typed[typed.len() - 10..]), "the end of the line shows: {shown:?}");
        assert!(shown.starts_with("C:\\work> "), "the prefix stays: {shown:?}");
    }

    #[test]
    fn a_short_line_is_drawn_as_is() {
        let (shown, cursor) = drawn_line("C:\\work", &line_with("dir"), 40);
        assert_eq!(shown.trim_end(), "C:\\work> dir");
        assert_eq!(cursor, "C:\\work> dir".len() as u16);
    }

    /// A deep directory left no room to type: the prefix keeps its end.
    #[test]
    fn a_long_prefix_is_shortened_from_the_left() {
        let cwd = format!("C:\\{}\\project", "very_long_directory_name\\".repeat(4));
        let (shown, cursor) = drawn_line(&cwd, &line_with("cargo build"), 60);

        assert!(shown.starts_with('…'), "{shown:?}");
        assert!(shown.contains("project> cargo build"), "{shown:?}");
        assert!(cursor < 60);
    }

    fn history_of(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("command_{i:04}")).collect()
    }

    fn rendered(menu: &CommandHistoryMenu, history: &[String], style: PopupStyle) -> String {
        let theme = Theme::dark();
        crate::test_support::render_text(80, 24, |frame| draw_command_history(frame, frame.area(), menu, history, "", &theme, style))
    }

    /// A selection deep in a long history stays on screen.
    #[test]
    fn selecting_an_entry_far_down_a_long_history_scrolls_it_into_view() {
        let history = history_of(200);
        let menu = CommandHistoryMenu { selected: 150 };

        let text = rendered(&menu, &history, PopupStyle::Classic);

        assert!(text.contains("command_0150"), "the selected entry should be scrolled into view:\n{text}");
    }

    #[test]
    fn a_short_history_needs_no_scrolling_to_show_the_selection() {
        let history = history_of(3);
        let menu = CommandHistoryMenu { selected: 2 };

        let text = rendered(&menu, &history, PopupStyle::Classic);

        assert!(text.contains("command_0002"));
    }

    /// The popup doesn't grow with the history: its bottom border lands on the
    /// same row for a short and a long one.
    #[test]
    fn the_popup_stays_a_fixed_height_regardless_of_history_length() {
        let short = history_of(3);
        let long = history_of(200);
        let menu = CommandHistoryMenu { selected: 0 };

        let bottom_border_row = |text: &str| text.lines().position(|line| line.trim().starts_with('└')).expect("popup should have a bottom border somewhere");

        assert_eq!(
            bottom_border_row(&rendered(&menu, &short, PopupStyle::Classic)),
            bottom_border_row(&rendered(&menu, &long, PopupStyle::Classic)),
            "the popup should close at the same row regardless of how long the history is"
        );
    }

    /// Regression coverage for the actual reported bug: unlike every
    /// other popup, this one never actually built on `popup::draw_frame`
    /// at all -- switching F9 -> Options -> UI to `Rounded` visibly did
    /// nothing for it. `Rounded`'s own chrome shows a rounded corner
    /// glyph (never `Classic`'s square `└`) and a separator right before
    /// the footer hints, same as `ui/find_file.rs`'s own results popup.
    #[test]
    fn rounded_style_shows_rounded_corners_and_a_separator_before_the_hints() {
        let history = history_of(2);
        let menu = CommandHistoryMenu { selected: 0 };

        let text = rendered(&menu, &history, PopupStyle::Rounded);

        assert!(text.contains('╰'), "should use Rounded's own corner glyph, not Classic's square one: {text}");
        let hint_line_index = text.lines().position(|line| line.contains("run")).expect("hint row should render");
        let line_above = text.lines().nth(hint_line_index - 1).unwrap();
        assert!(line_above.contains('─'), "a separator should sit right above the footer hints: {line_above:?}");
    }

    /// Both `PopupStyle`s should render the same history content -- only
    /// the chrome (border/padding/title placement) differs.
    #[test]
    fn classic_style_still_shows_the_history_and_hints() {
        let history = history_of(2);
        let menu = CommandHistoryMenu { selected: 0 };

        let text = rendered(&menu, &history, PopupStyle::Classic);

        assert!(text.contains("command_0000"), "history entries should be visible: {text}");
        assert!(text.contains("run"), "hint row should be visible: {text}");
        assert!(text.contains('└'), "should use Classic's own square corner: {text}");
    }
}
