use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};

use crate::notice::{Notice, NoticeKind};
use crate::theming::{PopupStyle, Theme};

/// Rows kept clear at the bottom: the command line and the F-key bar.
const BOTTOM_ROWS: u16 = 2;

const MIN_WIDTH: u16 = 24;


/// Draws `notice` as a toast in the bottom-right corner, above the bottom
/// rows, over whatever is on screen: a dot in the kind's color, then the
/// text, word-wrapped to at most half the terminal width. Skipped when
/// the terminal is too small to hold it.
pub(super) fn draw_notice(frame: &mut Frame, notice: &Notice, theme: &Theme, style: PopupStyle) {
    let area = frame.area();
    let Some(rect) = toast_rect(area, &notice.text) else {
        return;
    };
    let text_width = rect.width.saturating_sub(4) as usize;
    let color = kind_color(notice.kind, theme);

    let lines: Vec<Line> = wrap(&format!("\u{25cf} {}", notice.text), text_width)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            if index == 0 {
                let rest: String = row.chars().skip(2).collect();
                Line::from(vec![Span::styled("\u{25cf} ", Style::default().fg(color).add_modifier(Modifier::BOLD)), Span::styled(rest, Style::default().fg(theme.text))])
            } else {
                Line::from(Span::styled(row, Style::default().fg(theme.text)))
            }
        })
        .collect();

    let border_type = match style {
        PopupStyle::Rounded => BorderType::Rounded,
        PopupStyle::Classic => BorderType::Plain,
    };
    let block = Block::default().borders(Borders::ALL).border_type(border_type).border_style(Style::default().fg(color)).padding(Padding::horizontal(1));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}


fn kind_color(kind: NoticeKind, theme: &Theme) -> Color {
    match kind {
        NoticeKind::Info => theme.accent,
        NoticeKind::Error => theme.danger,
    }
}


/// The toast's rectangle: as wide as the text needs (plus the dot,
/// borders and a space on each side), capped at half the width;
/// one column off the right edge, right above the bottom rows. `None` if
/// it doesn't fit.
fn toast_rect(area: Rect, text: &str) -> Option<Rect> {
    let max_width = (area.width / 2).max(MIN_WIDTH).min(area.width.saturating_sub(1));
    if max_width < MIN_WIDTH {
        return None;
    }
    let wanted = text.chars().count() as u16 + 2 + 4;
    let width = wanted.clamp(MIN_WIDTH, max_width);
    let rows = wrap(&format!("\u{25cf} {text}"), width.saturating_sub(4) as usize).len() as u16;
    let available = area.height.saturating_sub(BOTTOM_ROWS);
    let height = (rows + 2).min(available);
    if height < 3 {
        return None;
    }
    let x = area.right().saturating_sub(width + 1);
    let y = area.top() + available - height;
    Some(Rect::new(x, y, width, height))
}


/// Greedy word wrap to `width` characters; a word longer than a line is
/// split across lines.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let current_len = current.chars().count();
            let needed = if current.is_empty() { word.len() } else { current_len + 1 + word.len() };
            if needed <= width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.extend(word.iter());
                break;
            }
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                continue;
            }
            let rest = word.split_off(width);
            rows.push(word.iter().collect());
            word = rest;
            if word.is_empty() {
                break;
            }
        }
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::test_support::buffer_text;

    fn rendered(notice: &Notice, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw_notice(frame, notice, &Theme::dark(), PopupStyle::Rounded)).unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn wrap_breaks_on_words_and_splits_an_overlong_one() {
        assert_eq!(wrap("delete failed: file in use", 12), vec!["delete", "failed: file", "in use"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 10), vec![""]);
    }

    #[test]
    fn the_toast_sits_bottom_right_above_the_last_two_rows() {
        let rect = toast_rect(Rect::new(0, 0, 80, 24), "Saved").unwrap();
        assert_eq!(rect.right(), 79, "one column off the right edge");
        assert_eq!(rect.bottom(), 22, "the command line and F-key rows stay visible");
    }

    #[test]
    fn a_long_message_wraps_within_half_the_width() {
        let text = "Delete failed: some-very-long-file-name.txt: The process cannot access the file because it is being used by another process.";
        let rect = toast_rect(Rect::new(0, 0, 80, 24), text).unwrap();
        assert_eq!(rect.width, 40);
        assert!(rect.height > 3);

        let screen = rendered(&Notice::error(text), 80, 24);
        assert!(screen.contains("Delete failed:"));
        assert!(screen.contains("process."), "the tail of the message is shown too: {screen}");
    }

    #[test]
    fn nothing_is_drawn_on_a_tiny_terminal() {
        assert_eq!(toast_rect(Rect::new(0, 0, 20, 4), "Saved"), None);
        let screen = rendered(&Notice::info("Saved"), 20, 4);
        assert!(!screen.contains("Saved"));
    }
}
