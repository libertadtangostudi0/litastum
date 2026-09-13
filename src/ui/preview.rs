use std::path::Path;

use ratatui::{
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders},
    Frame,
};

use crate::theming::Theme;

/// Shared chrome for `F3`'s two full-panel previews (`ui::image_preview`,
/// `ui::markdown_preview`) -- pulled out once both had grown the exact
/// same `Block::default().borders(ALL).border_style(theme.accent).title(...)`
/// boilerplate independently, the same "shared primitive" pattern
/// `ui::popup::draw_frame` already established for popup chrome
/// (`.claude/rules/litastum-popup-design.md`).
///
/// `theme.accent` -- the same color `draw_panel` uses for the *active*
/// panel's own border -- since `F3` always switches focus to the right
/// panel before either preview ever draws (`image_preview::open_preview`/
/// `markdown_preview::open_preview`'s own doc comments). `title` names
/// the file being previewed (its own file name, not the full path --
/// the panel is usually too narrow for one, and the directory is
/// already visible in the file listing this preview replaced); `title_bottom`
/// is `None` for the image preview (nothing to report there) and the
/// Markdown preview's own link-click status line when present. Returns
/// the inner `Rect` the caller draws its actual content into.
pub fn draw_preview_frame<'a>(frame: &mut Frame, area: Rect, theme: &Theme, title: impl Into<Line<'a>>, title_bottom: Option<Line<'a>>) -> Rect {
    let mut block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.accent)).title(title);
    if let Some(footer) = title_bottom {
        block = block.title_bottom(footer);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// The file-name portion of `path` (empty string if it has none) --
/// what both previews show as their own border title, never the full
/// path.
pub fn file_title(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::theming::Theme;

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        (0..area.height).map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn file_title_returns_just_the_file_name() {
        assert_eq!(file_title(Path::new("/some/dir/photo.png")), "photo.png");
    }

    #[test]
    fn file_title_is_empty_for_a_path_with_no_file_name() {
        assert_eq!(file_title(Path::new("/")), "");
    }

    #[test]
    fn draws_the_title_and_returns_an_inset_inner_rect() {
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut inner = Rect::default();
        terminal.draw(|frame| inner = draw_preview_frame(frame, frame.area(), &theme, Line::raw("photo.png"), None)).unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("photo.png"));
        assert_eq!(inner, Rect::new(1, 1, 18, 8), "border should inset the content area by one cell on every side");
    }

    #[test]
    fn draws_the_bottom_title_when_given_one() {
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_preview_frame(frame, frame.area(), &theme, Line::raw("readme.md"), Some(Line::raw("Opened: link")));
        })
        .unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("readme.md"));
        assert!(text.contains("Opened: link"));
    }
}
