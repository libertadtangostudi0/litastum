use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

use crate::explorer::{wrap_markdown_line, MarkdownLine, MarkdownLinkSearchState, MarkdownPreviewState, MarkdownSpan, MarkdownSpanKind};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;
use crate::ui::preview::{draw_preview_frame, file_title};

/// Renders `F3`'s currently-linked Markdown preview (`App::markdown_edit_preview`)
/// into `area` -- replaces the right panel's own file listing entirely
/// while it's `Some` (alongside the built-in editor in the left panel,
/// `Mode::Editing`/`ConfirmDiscard`/`MarkdownLinkSearch`). Border/title
/// chrome comes from `ui::preview::draw_preview_frame`, shared with
/// `ui::image_preview::draw_image_preview` (same active-panel border
/// styling for both).
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
    let title = file_title(state.path());
    let footer = state.link_message().map(str::to_string).unwrap_or_else(|| "l: search links  Ctrl+click: open one directly".to_string());
    let footer_style = if state.link_message().is_some() { Style::default().fg(theme.warning) } else { Style::default().fg(theme.text_dim) };
    let inner = draw_preview_frame(frame, area, theme, Line::raw(title), Some(Line::styled(footer, footer_style)));
    state.set_content_area(inner.x, inner.y, inner.width, inner.height);

    let width = inner.width as usize;
    let height = inner.height as usize;
    let start = state.scroll().min(state.lines().len());

    // Word-wrap logical lines into visual rows, one logical line at a
    // time, stopping once there's enough to fill the visible area --
    // no point wrapping the entire rest of a long document just to
    // throw most of it away below. `highlighted` tracks, in lockstep,
    // whether each pushed visual row belongs to the one logical line
    // `state.highlighted_line()` named -- every wrapped row of a
    // highlighted multi-row paragraph gets painted, not just its first.
    let highlighted_line = state.highlighted_line();
    let mut rows: Vec<MarkdownLine> = Vec::new();
    let mut highlighted: Vec<bool> = Vec::new();
    for (offset, line) in state.lines()[start..].iter().enumerate() {
        if rows.len() >= height {
            break;
        }
        let is_highlighted = highlighted_line == Some(start + offset);
        let wrapped = wrap_markdown_line(line, width);
        highlighted.extend(std::iter::repeat(is_highlighted).take(wrapped.len()));
        rows.extend(wrapped);
    }
    rows.truncate(height);
    highlighted.truncate(height);

    let row_links: Vec<Vec<(u16, u16, String)>> = rows.iter().map(|row| row_link_hitboxes(row)).collect();
    state.set_visible_row_links(row_links);

    let lines: Vec<Line> = rows.iter().zip(&highlighted).map(|(row, &hl)| render_line(row, theme, hl)).collect();
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
/// file-type coloring in `ui.rs`'s own `build_list_item`. `highlighted`
/// paints every span's background with `theme.current_row_bg` on top of
/// its own kind-based color -- the row `MarkdownPreviewState::sync_to_editor_cursor`
/// matched to the built-in editor's own cursor line, same "row
/// background, not a full-width fill" convention `ui/mod.rs::build_list_item`
/// already uses for a panel's own selected row (this codebase never
/// pads a line out to its column width just to color the rest of it).
fn render_line<'a>(line: &'a [MarkdownSpan], theme: &Theme, highlighted: bool) -> Line<'a> {
    if line.is_empty() {
        return Line::default();
    }
    Line::from(
        line.iter()
            .map(|span| {
                let mut style = span_style(span.kind, theme);
                if highlighted {
                    style = style.bg(theme.current_row_bg);
                }
                Span::styled(span.text.as_str(), style)
            })
            .collect::<Vec<_>>(),
    )
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

    /// The actual point of the whole sync feature: the rendered row
    /// matching `MarkdownPreviewState::sync_to_editor_cursor`'s own
    /// target line gets a `theme.current_row_bg` background, and only
    /// that line -- confirms `render_line`'s `highlighted` flag is
    /// actually reaching the real buffer, not just unit-tested against
    /// a hand-built `Style` in isolation.
    #[test]
    fn the_synced_line_is_painted_with_the_current_row_background() {
        use crate::explorer::MarkdownPreviewState;
        use crate::test_support::unique_scratch_dir;
        use std::fs;

        let dir = unique_scratch_dir("markdown-preview-highlight");
        let path = dir.join("readme.md");
        fs::write(&path, "first\n\nsecond\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        // Syncs to "first" (source line 0) rather than "second" --
        // deliberately so `scroll` stays at 0 and *both* lines remain
        // on screen at their usual rows, letting this test contrast a
        // highlighted row against a genuinely-rendered unhighlighted
        // one instead of one that's merely scrolled out of view.
        state.sync_to_editor_cursor(0, 0.0, 22);

        let backend = TestBackend::new(20, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| draw_markdown_preview(frame, frame.area(), &mut state, &theme)).unwrap();

        let buffer = terminal.backend().buffer();
        let first_row_bg = buffer[(1, 1)].bg;
        let second_row_bg = buffer[(1, 3)].bg;
        assert_eq!(first_row_bg, theme.current_row_bg, "the synced line (\"first\") should carry the highlight");
        assert_ne!(second_row_bg, theme.current_row_bg, "an unrelated line (\"second\") should not");
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
