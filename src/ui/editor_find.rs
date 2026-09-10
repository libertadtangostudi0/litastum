use ratatui::{
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear},
    Frame,
};

use crate::editor::Editor;
use crate::theming::Theme;

/// Margin between the popup and the editor area's own top/right edges.
const MARGIN: u16 = 1;

/// Total popup width, border included -- deliberately compact per
/// explicit request: just the border and the query field inside, no
/// title/label ("this is standard editor behavior, doesn't need
/// spelling out") and no footer hints.
const WIDTH: u16 = 30;

/// Renders the `Ctrl+F` search box -- reported too large and too
/// labeled on a first pass (used the same chrome as the F8 delete
/// popup, `popup::draw_frame`, with a dot+title row and a corner
/// badge): redone as a single-row bordered field, anchored to the
/// editor area's own top-right corner rather than centered, matching
/// how VS Code/Sublime's own find widgets are positioned. Returns
/// where the real terminal cursor should sit, same mechanism as the
/// command line's own cursor (`ui::draw`).
pub fn draw_find_popup(frame: &mut Frame, area: Rect, editor: &Editor, search_history: &[String], theme: &Theme) -> Position {
    let width = WIDTH.min(area.width);
    let popup = Rect {
        x: area.right().saturating_sub(width + MARGIN).max(area.x),
        y: area.y + MARGIN,
        width,
        height: 3,
    };
    frame.render_widget(Clear, popup);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let query = editor.search_query();
    // Char-based, not a byte slice -- unlike a shell command line, a
    // search query can easily contain non-ASCII text (searching for a
    // Cyrillic string, a Unicode identifier, ...), where slicing by
    // `query.len()` (byte count) could land mid-character and panic.
    let suggestion_suffix = crate::editor::find_history::suggest(search_history, &query)
        .map(|full| full.chars().skip(query.chars().count()).collect::<String>());

    let mut spans = vec![Span::styled(query.clone(), Style::default().fg(theme.text))];
    if let Some(suffix) = suggestion_suffix {
        spans.push(Span::styled(suffix, Style::default().fg(theme.text_dim)));
    }
    frame.render_widget(Line::from(spans), inner);

    Position { x: inner.x + query.chars().count() as u16, y: inner.y }
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::editor::Editor;
    use crate::test_support::unique_scratch_dir;

    fn open_test_editor(contents: &str) -> Editor {
        let path = unique_scratch_dir("editor-find-popup").join("file.txt");
        std::fs::write(&path, contents).expect("write test fixture file");
        Editor::open(path, None).expect("open test fixture file")
    }

    #[test]
    fn draw_find_popup_shows_the_query_and_places_the_cursor_after_it() {
        let mut editor = open_test_editor("hello world");
        editor.start_search();
        editor.search_push_char('w');
        editor.search_push_char('o');
        let theme = Theme::dark();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut cursor = Position::default();
        terminal
            .draw(|frame| {
                cursor = draw_find_popup(frame, frame.area(), &editor, &[], &theme);
            })
            .unwrap();

        let contents = buffer_text(terminal.backend().buffer());
        assert!(contents.contains("wo"));
        assert!(cursor.x > 0);
    }

    #[test]
    fn draw_find_popup_shows_a_dimmed_suggestion_suffix() {
        let mut editor = open_test_editor("hello world");
        editor.start_search();
        editor.search_push_char('w');
        let theme = Theme::dark();
        let history = vec!["world".to_string()];

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_find_popup(frame, frame.area(), &editor, &history, &theme);
            })
            .unwrap();

        let contents = buffer_text(terminal.backend().buffer());
        assert!(contents.contains("world"), "typed 'w' plus the dimmed 'orld' suffix should together read 'world'");
    }

    /// Real requirement, stated directly: the popup goes in the top-right
    /// corner, not centered.
    #[test]
    fn draw_find_popup_sits_in_the_top_right_corner() {
        let editor = open_test_editor("hello world");
        let theme = Theme::dark();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let area = Rect::new(0, 0, 80, 24);
        terminal
            .draw(|frame| {
                draw_find_popup(frame, area, &editor, &[], &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        // The popup's own right border sits `MARGIN` cells in from the
        // area's own right edge, at row `MARGIN` from the top -- not
        // centered in either axis.
        let right_border_x = area.right() - 1 - MARGIN;
        assert!(buffer[(right_border_x, MARGIN)].symbol() != " ", "should have a border cell near the top-right corner");
        assert_eq!(buffer[(area.width / 2, area.height / 2)].symbol(), " ", "should not be drawn centered");
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
