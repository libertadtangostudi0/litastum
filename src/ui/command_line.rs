use std::path::Path;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::command_line::{matching_history, CommandHistoryMenu};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// The always-live command line (Far Manager-style — see
/// `command_line.rs`). Shows the active shell profile's name at the
/// right edge, since which one a typed command actually runs against
/// is otherwise invisible (`Ctrl+P` to change it).
/// Renders `"{cwd}> {typed}"`, matching real Far Manager's own command
/// line (which always shows the active panel's path, not just a bare
/// `>` prompt with no indication of where a command would actually
/// run). Returns the prefix's character count — `ui::draw` needs it to
/// place the real terminal cursor right after the typed text, since
/// that position now depends on `cwd`'s length, not a fixed `"> "`.
///
/// No shell-profile-name hint on the right edge anymore (an earlier
/// version had one, `"Ctrl+P {shell_name}"`) — reported as visual
/// clutter that doesn't belong on a Far-style command line; `Ctrl+P`'s
/// own picker already shows which profile is active when opened.
pub(super) fn draw_command_line(frame: &mut Frame, area: Rect, cwd: &Path, command_line: &str, selection_anchor: Option<usize>, cursor: usize, theme: &Theme) -> u16 {
    let prefix = format!("{}> ", cwd.display());

    let mut spans = vec![Span::styled(prefix.clone(), Style::default().fg(theme.command_line_prefix).add_modifier(Modifier::BOLD))];

    // A `Shift`/`Ctrl+Shift`+`Left`/`Right` selection (`command_line/
    // browsing.rs`) highlights the same way the Copy/Move destination
    // field's own selection does (`text_field.rs::destination_line`,
    // `theme.current_row_bg`) -- no selection just renders as one plain
    // span, same as before this feature existed.
    match selection_anchor {
        Some(anchor) => {
            let (start, end) = crate::text_field::selection_range(anchor, cursor);
            let chars: Vec<char> = command_line.chars().collect();
            let before: String = chars[..start].iter().collect();
            let selected: String = chars[start..end].iter().collect();
            let after: String = chars[end..].iter().collect();
            spans.push(Span::styled(before, Style::default().fg(theme.text)));
            spans.push(Span::styled(selected, popup::selected_text_style(theme)));
            spans.push(Span::styled(after, Style::default().fg(theme.text)));
        }
        None => spans.push(Span::styled(command_line.to_string(), Style::default().fg(theme.text))),
    }

    frame.render_widget(Line::from(spans), area);
    prefix.chars().count() as u16
}

/// Fixed popup height, independent of `history.len()`/how many entries
/// currently match `query` -- the list itself now scrolls (see the
/// `ListState` below) to keep the selected entry in view, so there's no
/// reason for the popup itself to keep growing with the match count the
/// way it used to. Same value `ui/find_file.rs::RESULTS_HEIGHT` already
/// settled on, for the same "fixed-height, scrollable list popup"
/// shape. Widened from 60 to 70 columns alongside this -- a real typed
/// command (a long `svn`/`git` invocation, a deep path) routinely runs
/// past 60 columns and used to get clipped mid-line with no way to see
/// the rest.
const HISTORY_HEIGHT: u16 = 21;
const HISTORY_WIDTH: u16 = 70;

/// Renders the History popup — filtered live against `query` (the same
/// always-live command line everything else types into, per
/// `command_line::matching_history`'s doc), most-recently-run command
/// last within the filtered list (natural reading order for "what did
/// I just type"), or a hint that nothing matches (or nothing's been
/// run yet, if history itself is empty).
///
/// `style` (F9 -> Options -> UI) went unused here until now -- reported
/// directly (the Rounded style did nothing for this one popup): unlike
/// every other popup (`ui::draw`'s own match arm always passes
/// `app.popup_style` through), this one hand-rolled a fixed, `Classic`-only
/// `Block::borders(ALL)` instead of building on `popup::draw_frame`,
/// so switching to `Rounded` visibly did nothing for it. Missed during
/// the original `ui/popup.rs` migration pass (`TODO/code-quality.md`)
/// -- every popup that migration pass actually touched picked it up,
/// this one just wasn't on the list.
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
        // A real command history (or a broad query matching most of it)
        // used to render every entry straight into this fixed-height
        // area with no scroll offset at all -- reported directly, with
        // a screenshot showing the actually-selected entry clipped off
        // past the popup's own bottom border, nowhere to be seen. Same
        // "`List` with no `ListState` doesn't auto-scroll" gap already
        // fixed this way for `ui/find_file.rs`'s own results list and
        // `ui/theme_menu.rs`'s picker -- a real `ListState` tracking the
        // selected index, which `List` then scrolls to keep in view on
        // its own.
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

/// Renders the auto-popping history-suggestion list
/// (`command_line::suggest_history`) directly above `command_line_area`
/// (the always-live command line's own row) — unlike `draw_command_history`
/// above, this isn't a centered modal popup: it appears unprompted the
/// instant there's at least one match, Far Manager's own command-line
/// autocomplete behaves the same way. `Up`/`Down` move `selected`,
/// `Tab` accepts it into the command line (both `browsing.rs`, ahead of
/// their usual panel-navigation/path-completion meaning while this
/// list is actually showing).
pub fn draw_history_suggestions(frame: &mut Frame, command_line_area: Rect, suggestions: &[&str], selected: usize, theme: &Theme) {
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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" History ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<ListItem> = suggestions
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let style = if index == selected {
                popup::selected_row_style(theme)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(entry.to_string(), style)))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::command_line::CommandHistoryMenu;
    use crate::theming::Theme;

    fn history_of(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("command_{i:04}")).collect()
    }

    fn rendered(menu: &CommandHistoryMenu, history: &[String], style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| draw_command_history(frame, frame.area(), menu, history, "", &theme, style)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Regression test for the actual reported bug: a history far past
    /// what the fixed-height popup can show at once used to render
    /// every entry into the list with no scroll offset at all -- the
    /// selected row, deep into a long history, was simply invisible,
    /// clipped off past the popup's own bottom border with nothing to
    /// bring it into view. A real `ListState` (see the call site's own
    /// doc comment) should keep whichever row is selected actually on
    /// screen no matter how far into a long history it is.
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

    /// Regression test for the other half of the same report: the
    /// popup used to grow with the match count instead of staying a
    /// fixed size -- a long history made the whole popup grow to fill
    /// (and, past the terminal's own height, overflow) the screen.
    /// Checked by finding which *row* the popup's own bottom border
    /// lands on: a fixed-height popup closes at the same row regardless
    /// of history length, a growing one closes further down (or off
    /// the bottom entirely) for the longer history.
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
