use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

use crate::text_field::TextField;

/// What's typed while a command runs. Every key goes to the program; the
/// text is also shown in our command line, after whatever was there. When
/// the program ends, the text stays there -- as `cmd` keeps type-ahead for
/// its next prompt -- unless the program read it, which shows as its echo
/// in the output. A line sent with `Enter` is the program's and goes
/// (a password prompt doesn't echo; it must not come back). Reported: the
/// next command typed during a run was lost.
pub(super) struct TypeAhead {
    /// Where the typed text starts in the command line, in characters.
    start: usize,
    /// The output line the cursor was on when the current text began --
    /// the program's echo of it can't be above that.
    echo_from: Option<usize>,
}

impl TypeAhead {
    pub fn begin(line: &mut TextField) -> Self {
        line.move_to_end();
        Self { start: line.text().chars().count(), echo_from: None }
    }

    /// Mirrors `key`, already sent to the program, into `line`.
    /// `output_line`: the program's cursor line now.
    pub fn key(&mut self, line: &mut TextField, key: KeyEvent, output_line: usize) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(c) if !ctrl && !alt => {
                self.echo_from.get_or_insert(output_line);
                line.insert_char(c);
            }
            KeyCode::Backspace if line.text().chars().count() > self.start => line.backspace(),
            KeyCode::Enter => self.drop_typed(line),
            KeyCode::Char('c' | 'C') if ctrl => self.drop_typed(line),
            _ => {}
        }
    }

    /// Mirrors pasted text: lines ending in a break were sent; the rest
    /// is being typed.
    pub fn paste(&mut self, line: &mut TextField, text: &str, output_line: usize) {
        let text = text.replace('\r', "");
        let typed = match text.rsplit_once('\n') {
            Some((_, rest)) => {
                self.drop_typed(line);
                rest
            }
            None => text.as_str(),
        };
        for c in typed.chars() {
            self.echo_from.get_or_insert(output_line);
            line.insert_char(c);
        }
    }

    /// The program ended with `output`: the typed text stays in `line`
    /// unless it shows up in the output since it was typed.
    pub fn finish(mut self, line: &mut TextField, output: &[Line]) {
        let typed: String = line.text().chars().skip(self.start).collect();
        if typed.is_empty() {
            return;
        }
        let from = self.echo_from.unwrap_or(0).min(output.len());
        let echoed = output[from..].iter().any(|row| row.to_string().contains(&typed));
        if echoed {
            self.drop_typed(line);
        }
    }

    fn drop_typed(&mut self, line: &mut TextField) {
        let kept: String = line.text().chars().take(self.start).collect();
        line.set_text(kept);
        self.echo_from = None;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    fn type_text(type_ahead: &mut TypeAhead, line: &mut TextField, text: &str, output_line: usize) {
        for c in text.chars() {
            type_ahead.key(line, key(KeyCode::Char(c)), output_line);
        }
    }

    /// Reported: the next command typed during `dir` was lost.
    #[test]
    fn text_the_program_never_read_stays_in_the_command_line() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text(&mut type_ahead, &mut line, "svn sx", 3);
        type_ahead.key(&mut line, key(KeyCode::Backspace), 3);
        type_text(&mut type_ahead, &mut line, "t", 3);

        type_ahead.finish(&mut line, &[Line::raw("a.txt"), Line::raw("b.txt"), Line::raw("c.txt"), Line::raw("2 File(s)")]);

        assert_eq!(line.text(), "svn st");
    }

    #[test]
    fn text_the_program_echoed_was_read_and_goes() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text(&mut type_ahead, &mut line, "yes", 1);

        type_ahead.finish(&mut line, &[Line::raw("yes or no?"), Line::raw("Answer: yes")]);

        assert_eq!(line.text(), "", "the echo after the typing began");
    }

    #[test]
    fn an_echo_above_where_the_typing_began_doesnt_count() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text(&mut type_ahead, &mut line, "dir", 2);

        type_ahead.finish(&mut line, &[Line::raw("C:> dir"), Line::raw("listing"), Line::raw("done")]);

        assert_eq!(line.text(), "dir");
    }

    /// A password typed at a prompt doesn't echo: a sent line never comes
    /// back.
    #[test]
    fn a_line_sent_with_enter_never_comes_back() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text(&mut type_ahead, &mut line, "secret", 0);
        type_ahead.key(&mut line, key(KeyCode::Enter), 0);
        type_text(&mut type_ahead, &mut line, "next", 1);

        type_ahead.finish(&mut line, &[Line::raw("Password:"), Line::raw("ok")]);

        assert_eq!(line.text(), "next");
    }

    #[test]
    fn earlier_text_and_backspace_stay_out_of_the_way() {
        let mut line = TextField::with_text("cd ");
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_ahead.key(&mut line, key(KeyCode::Backspace), 0);
        assert_eq!(line.text(), "cd ", "Backspace doesn't reach what was there before");

        type_ahead.paste(&mut line, "first\r\nsrc", 0);
        assert_eq!(line.text(), "cd src", "a pasted line ending in a break was sent");

        type_ahead.key(&mut line, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), 0);
        assert_eq!(line.text(), "cd ", "Ctrl+C drops the typed text");
    }
}
