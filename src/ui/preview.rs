use std::path::Path;

use ratatui::{
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders},
    Frame,
};

use crate::theming::Theme;

/// Shared frame for the F3 previews (image, Markdown): `theme.accent`
/// border, since F3 makes the right panel active; the file name as the
/// title (the path is in the listing it replaced); an optional bottom
/// title (the Markdown link status). Returns the inner area.
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
    use crate::test_support::buffer_text;
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::theming::Theme;


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
