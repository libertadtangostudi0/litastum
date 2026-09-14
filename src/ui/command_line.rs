use std::path::Path;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::command_line::{matching_history, CommandHistoryMenu};
use crate::theming::Theme;
use crate::ui::centered_rect;
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

/// Renders the History popup — filtered live against `query` (the same
/// always-live command line everything else types into, per
/// `command_line::matching_history`'s doc), most-recently-run command
/// last within the filtered list (natural reading order for "what did
/// I just type"), or a hint that nothing matches (or nothing's been
/// run yet, if history itself is empty).
pub fn draw_command_history(frame: &mut Frame, area: Rect, menu: &CommandHistoryMenu, history: &[String], query: &str, theme: &Theme) {
    let matches = matching_history(history, query);
    let height = (matches.len().max(1) as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(60, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" History ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

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
        frame.render_widget(List::new(items), rows[0]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" select  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
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
