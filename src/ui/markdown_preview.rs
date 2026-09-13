use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::explorer::{MarkdownLinkSearchState, MarkdownPreviewState, MarkdownSpan, MarkdownSpanKind};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders `F3`'s currently-previewed Markdown file into `area` --
/// replaces the right panel's own file listing entirely while
/// `Mode::MarkdownPreview` is active, same convention
/// `ui::image_preview::draw_image_preview` already established for the
/// image case (same active-panel border styling too). Only the lines
/// from `state.scroll()` onward are actually laid out -- `Paragraph`
/// itself has no concept of "start partway through a `Vec<Line>`", so
/// this slices `state.lines()` first rather than handing it the whole
/// document and a scroll offset it has no matching parameter for.
///
/// Records `inner` on `state` (`set_content_area`) every frame -- the
/// one place that actually knows where the content landed on screen,
/// needed by `explorer::markdown_preview::handle_markdown_preview_mouse`
/// to turn a raw mouse click position back into "which rendered line."
///
/// The bottom border shows `state.link_message()` (what the *last*
/// `Ctrl`+click actually did -- opened, failed, or found nothing to
/// open) once there's been one, or a static "how to do this at all"
/// hint before that -- added after `Ctrl`+click was reported as giving
/// no visible feedback at all, indistinguishable from not working.
pub fn draw_markdown_preview(frame: &mut Frame, area: Rect, state: &mut MarkdownPreviewState, theme: &Theme) {
    let title = state.path().file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let footer = state.link_message().map(str::to_string).unwrap_or_else(|| "l: search links  Ctrl+click: open one directly".to_string());
    let footer_style = if state.link_message().is_some() { Style::default().fg(theme.warning) } else { Style::default().fg(theme.text_dim) };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(title)
        .title_bottom(Line::styled(footer, footer_style));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    state.set_content_area(inner.x, inner.y, inner.width, inner.height);

    let start = state.scroll().min(state.lines().len());
    let lines: Vec<Line> = state.lines()[start..].iter().map(|line| render_line(line, theme)).collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}


/// Renders `l`'s keyboard-driven link browser (`Mode::MarkdownLinkSearch`)
/// as a popup over the still-visible preview (`ui::draw`'s own
/// `Mode::MarkdownLinkSearch` arm draws `draw_markdown_preview`
/// underneath first) -- a query field, the filtered link list with the
/// selected one highlighted, and a footer hint, same shared chrome
/// (`ui/popup.rs`) every other popup in this app builds on. Returns
/// where the real terminal cursor should sit, same mechanism as every
/// other text-entry popup.
pub fn draw_markdown_link_search(frame: &mut Frame, area: Rect, search: &MarkdownLinkSearchState, theme: &Theme, style: PopupStyle) -> Position {
    let filtered = search.filtered();
    let extra = popup::chrome_extra_rows(style);
    let height = (filtered.len().max(1) as u16 + 6 + extra).clamp(8 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Links ")), 60, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(Line::from(Span::styled(search.query(), Style::default().fg(theme.text))), rows[0]);
    frame.render_widget(popup::separator(inner.width, theme), rows[1]);

    if filtered.is_empty() {
        frame.render_widget(Line::from(Span::styled("No matching links", Style::default().fg(theme.text_dim))), rows[2]);
    } else {
        let items: Vec<ListItem> = filtered
            .iter()
            .enumerate()
            .map(|(i, link)| {
                let row_style = if i == search.selected() {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(link.label.clone(), row_style)))
            })
            .collect();
        frame.render_widget(List::new(items), rows[2]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" open  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[4]);

    Position { x: rows[0].x + search.query().chars().count() as u16, y: rows[0].y }
}


/// Maps one `explorer::markdown_preview::MarkdownLine`'s spans into a
/// real `ratatui::text::Line`, styled per `span_style` below -- the
/// domain/rendering split `explorer::HighlightRole` already uses for
/// file-type coloring in `ui.rs`'s own `build_list_item`.
fn render_line<'a>(line: &'a [MarkdownSpan], theme: &Theme) -> Line<'a> {
    if line.is_empty() {
        return Line::default();
    }
    Line::from(line.iter().map(|span| Span::styled(span.text.as_str(), span_style(span.kind, theme))).collect::<Vec<_>>())
}

fn span_style(kind: MarkdownSpanKind, theme: &Theme) -> Style {
    match kind {
        MarkdownSpanKind::Plain => Style::default().fg(theme.text),
        MarkdownSpanKind::Heading(_) => Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        MarkdownSpanKind::Bold => Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        MarkdownSpanKind::Italic => Style::default().fg(theme.text).add_modifier(Modifier::ITALIC),
        MarkdownSpanKind::Code => Style::default().fg(theme.warning),
        MarkdownSpanKind::Link => Style::default().fg(theme.accent).add_modifier(Modifier::UNDERLINED),
        MarkdownSpanKind::Quote => Style::default().fg(theme.text_dim).add_modifier(Modifier::ITALIC),
        MarkdownSpanKind::Rule => Style::default().fg(theme.border),
    }
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::explorer::MarkdownLink;

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        (0..area.height).map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn shows_the_query_and_every_matching_link() {
        let mut search = MarkdownLinkSearchState::new(vec![
            MarkdownLink { label: "Anthropic".to_string(), url: "https://anthropic.com".to_string() },
            MarkdownLink { label: "Contributing".to_string(), url: "CONTRIBUTING.md".to_string() },
        ]);
        search.push_char('a');

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut cursor = Position::default();
        terminal.draw(|frame| cursor = draw_markdown_link_search(frame, frame.area(), &search, &theme, PopupStyle::Rounded)).unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Anthropic"));
        assert!(cursor.x > 0);
    }

    #[test]
    fn shows_a_hint_when_nothing_matches() {
        let mut search = MarkdownLinkSearchState::new(vec![MarkdownLink { label: "Anthropic".to_string(), url: "https://anthropic.com".to_string() }]);
        for c in "nonexistent".chars() {
            search.push_char(c);
        }

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| { draw_markdown_link_search(frame, frame.area(), &search, &theme, PopupStyle::Rounded); }).unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("No matching links"));
    }
}
