use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::explorer::{wrap_markdown_line, MarkdownLine, MarkdownLinkSearchState, MarkdownPreviewState, MarkdownSpan, MarkdownSpanKind};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders `F3`'s currently-previewed Markdown file into `area` --
/// replaces the right panel's own file listing entirely while
/// `Mode::MarkdownPreview` is active, same convention
/// `ui::image_preview::draw_image_preview` already established for the
/// image case (same active-panel border styling too).
///
/// Word-wraps each logical line *itself* (`explorer::wrap_markdown_line`)
/// rather than handing `ratatui`'s own `Paragraph` a `Wrap` to do it --
/// the two would look identical on screen, but only wrapping it here
/// means the exact same rows handed to `Paragraph` can also be used to
/// build `state`'s own click hitboxes (`set_visible_row_links`), so
/// rendering and mouse hit-testing can never disagree (an earlier
/// version approximated a click's row against `state.scroll()`
/// directly, without knowing where `ratatui`'s own wrap points landed,
/// and drifted once an earlier line had actually wrapped -- reported
/// directly against a real link that sat right after a long paragraph).
///
/// Records `inner` on `state` (`set_content_area`) every frame -- the
/// one place that actually knows where the content landed on screen,
/// needed by `explorer::markdown_preview::handle_markdown_preview_mouse`
/// to turn a raw mouse click position back into "which rendered row."
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

    let width = inner.width as usize;
    let height = inner.height as usize;
    let start = state.scroll().min(state.lines().len());

    // Word-wrap logical lines into visual rows, one logical line at a
    // time, stopping once there's enough to fill the visible area --
    // no point wrapping the entire rest of a long document just to
    // throw most of it away below.
    let mut rows: Vec<MarkdownLine> = Vec::new();
    for line in &state.lines()[start..] {
        if rows.len() >= height {
            break;
        }
        rows.extend(wrap_markdown_line(line, width));
    }
    rows.truncate(height);

    let row_links: Vec<Vec<(u16, u16, String)>> = rows.iter().map(|row| row_link_hitboxes(row)).collect();
    state.set_visible_row_links(row_links);

    let lines: Vec<Line> = rows.iter().map(|row| render_line(row, theme)).collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The `(column_start, column_end, url)` of every link on one already-
/// wrapped visual `row` -- `draw_markdown_preview`'s own hitbox table
/// for `MarkdownPreviewState::link_at`, built from exactly the row
/// that's about to be rendered.
fn row_link_hitboxes(row: &MarkdownLine) -> Vec<(u16, u16, String)> {
    let mut hitboxes = Vec::new();
    let mut column = 0u16;
    for span in row {
        let width = span.text.chars().count() as u16;
        if span.kind == MarkdownSpanKind::Link {
            if let Some(url) = &span.url {
                hitboxes.push((column, column + width, url.clone()));
            }
        }
        column += width;
    }
    hitboxes
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

    /// Integration coverage for the exact-hit-test fix: a paragraph
    /// long enough to actually wrap (forcing `ratatui`'s own automatic
    /// `Paragraph` wrap point to disagree with a naive `scroll() + row`
    /// lookup) still resolves a click on the link correctly after a
    /// real `draw_markdown_preview` call -- proving `visible_row_links`
    /// is wired all the way into rendering, not just unit-tested in
    /// isolation via `wrap_markdown_line`.
    #[test]
    fn a_link_after_a_wrapped_paragraph_is_still_clickable_after_a_real_draw() {
        use crate::explorer::MarkdownPreviewState;
        use crate::test_support::unique_scratch_dir;
        use std::fs;

        let dir = unique_scratch_dir("markdown-preview-draw");
        let path = dir.join("readme.md");
        let long_paragraph = "word ".repeat(20);
        fs::write(&path, format!("{long_paragraph}\n\n[Anthropic](https://anthropic.com)\n")).unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();

        let backend = TestBackend::new(20, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| draw_markdown_preview(frame, frame.area(), &mut state, &theme)).unwrap();

        // The long paragraph wraps across several rows on a 20-column
        // terminal -- find whichever row the link actually landed on
        // (borders take one column, so the click column stays at 1)
        // instead of assuming a fixed row number.
        let found = (0..24).any(|row| state.link_at(1, row) == Some("https://anthropic.com"));
        assert!(found, "the link should be clickable somewhere in the rendered output");
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
