//! The user screen, as in Far: what commands printed, kept by litastum
//! itself rather than left on the real terminal, so `Ctrl+O` shows it
//! with our own command line and popups over it.
//!
//! - `session`: a command running in a pseudoconsole, parsed into a grid;
//! - `grid`: grid rows as `ratatui` lines;
//! - `keys`: key presses as the bytes a terminal sends;
//! - `selection`: text selected with the mouse.
//!
//! Commands that ran are kept here as lines (`UserScreen`), each one set
//! apart from the one before by blank lines.

use std::collections::VecDeque;
use std::path::Path;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theming::Theme;

mod grid;
mod keys;
mod selection;
mod session;

pub use keys::encode_key;
pub use session::{LiveCommand, LiveView};

/// Lines kept at most; the oldest go first.
const MAX_LINES: usize = 20_000;

/// Blank lines between one command's output and the next prompt
/// (requested: three).
const SEPARATOR_LINES: usize = 3;


/// What commands printed, oldest line first.
#[derive(Debug, Default)]
pub struct UserScreen {
    lines: VecDeque<Line<'static>>,
    /// How many lines the `Ctrl+O` view is scrolled up from the bottom.
    scroll: usize,
    /// Rows the view had when last drawn -- a page for `PageUp`.
    visible_rows: usize,
    selection: Option<selection::Selection>,
}

impl UserScreen {
    pub fn lines(&self) -> &VecDeque<Line<'static>> {
        &self.lines
    }

    /// Before a command: blank lines after the previous output, then the
    /// prompt and the command, as typed.
    pub fn begin_command(&mut self, cwd: &Path, command: &str, theme: &Theme) {
        if !self.lines.is_empty() {
            for _ in 0..SEPARATOR_LINES {
                self.push(Line::default());
            }
        }
        let prompt = Style::default().fg(theme.command_line_prefix).add_modifier(Modifier::BOLD);
        self.push(Line::from(vec![Span::styled(format!("{}> ", cwd.display()), prompt), Span::styled(command.to_string(), Style::default().fg(theme.text))]));
        self.scroll = 0;
        self.selection = None;
    }

    /// A message of litastum's own (a failed launch, a missing `cd` target).
    pub fn push_message(&mut self, text: &str, theme: &Theme) {
        self.push(Line::styled(text.to_string(), Style::default().fg(theme.danger)));
    }

    /// A finished command's output.
    pub fn extend(&mut self, lines: Vec<Line<'static>>) {
        for line in lines {
            self.push(line);
        }
        self.scroll = 0;
        self.selection = None;
    }

    /// `cls`/`clear`.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
        self.selection = None;
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn visible_rows(&self) -> usize {
        self.visible_rows.max(1)
    }

    /// Written by the renderer each frame.
    pub fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows;
    }

    /// Scrolls the `Ctrl+O` view by `delta` lines (up is positive), within
    /// what's there.
    pub fn scroll_by(&mut self, delta: isize, visible_rows: usize) {
        let most = self.lines.len().saturating_sub(visible_rows);
        self.scroll = self.scroll.saturating_add_signed(delta).min(most);
    }

    fn push(&mut self, line: Line<'static>) {
        if self.lines.len() == MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn text(screen: &UserScreen) -> Vec<String> {
        screen.lines().iter().map(|line| line.to_string()).collect()
    }

    /// Requested: each command's output set apart by three blank lines.
    #[test]
    fn commands_are_set_apart_by_three_blank_lines() {
        let theme = Theme::dark();
        let mut screen = UserScreen::default();

        screen.begin_command(Path::new("W:\\x"), "dir", &theme);
        screen.extend(vec![Line::raw("a.txt")]);
        screen.begin_command(Path::new("W:\\x"), "svn st", &theme);

        assert_eq!(text(&screen), ["W:\\x> dir", "a.txt", "", "", "", "W:\\x> svn st"]);
    }

    #[test]
    fn the_first_command_starts_at_the_top_and_cls_empties_it() {
        let theme = Theme::dark();
        let mut screen = UserScreen::default();
        screen.begin_command(Path::new("W:\\x"), "dir", &theme);
        assert_eq!(text(&screen), ["W:\\x> dir"]);

        screen.clear();
        assert!(screen.lines().is_empty());
    }

    #[test]
    fn scrolling_stays_within_the_lines_and_new_output_goes_back_down() {
        let mut screen = UserScreen::default();
        screen.extend((0..10).map(|n| Line::raw(n.to_string())).collect());

        screen.scroll_by(100, 4);
        assert_eq!(screen.scroll(), 6, "the first line at the top, no further");
        screen.scroll_by(-2, 4);
        assert_eq!(screen.scroll(), 4);
        screen.scroll_by(-100, 4);
        assert_eq!(screen.scroll(), 0);

        screen.scroll_by(3, 4);
        screen.extend(vec![Line::raw("new")]);
        assert_eq!(screen.scroll(), 0);
    }

    #[test]
    fn the_oldest_lines_go_past_the_limit() {
        let mut screen = UserScreen::default();
        screen.extend((0..MAX_LINES + 2).map(|n| Line::raw(n.to_string())).collect());

        assert_eq!(screen.lines().len(), MAX_LINES);
        assert_eq!(screen.lines()[0].to_string(), "2");
    }
}
