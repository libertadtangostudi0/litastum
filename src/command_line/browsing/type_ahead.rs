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
///
/// A program seen reading -- the typed text echoed right before its
/// cursor (`observe`) -- gets the keys alone from then on: reported, a
/// paste into a script's prompt showed twice, there and in our line.
///
/// At a password prompt (`asks_for_secret`) the text shows as `*`s and
/// never stays in the line (reported: `hg pull`'s password showed in the
/// clear).
pub(super) struct TypeAhead {
    /// Where the typed text starts in the command line, in characters.
    start: usize,
    /// The output line the cursor was on when the current text began --
    /// the program's echo of it can't be above that.
    echo_from: Option<usize>,
    /// The program's line up to its cursor when the current text began:
    /// an echo has to change it.
    before_typing: Option<String>,
    /// The program reads what's typed: nothing is mirrored any more.
    program_reads: bool,
    /// The current text answers a password prompt: mirrored as `*`s.
    secret: bool,
}


/// Whether `before_cursor`, the program's line up to its cursor, asks for
/// a password -- `Password:`, `Password for 'https://...':`, `Enter
/// passphrase for key '...':`, `user@host's password:`. By the words,
/// since a program that turns echo off doesn't say so on the terminal.
pub(super) fn asks_for_secret(before_cursor: &str) -> bool {
    const WORDS: &[&str] = &["password", "passphrase", "passcode", "passwort", "kennwort", "пароль", "mot de passe", "pin"];
    let prompt = before_cursor.trim_end();
    if !prompt.ends_with(':') {
        return false;
    }
    let lower = prompt.to_lowercase();
    let is_letter = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    // Whole words: `pin` isn't in `shipping`.
    WORDS.iter().any(|word| lower.match_indices(word).any(|(at, _)| !is_letter(lower[..at].chars().next_back()) && !is_letter(lower[at + word.len()..].chars().next())))
}

impl TypeAhead {
    pub fn begin(line: &mut TextField) -> Self {
        line.move_to_end();
        Self { start: line.text().chars().count(), echo_from: None, before_typing: None, program_reads: false, secret: false }
    }

    /// Mirrors `key`, already sent to the program, into `line`.
    /// `output_line`: the program's cursor line now; `before_cursor`: that
    /// line's text up to the cursor.
    pub fn key(&mut self, line: &mut TextField, key: KeyEvent, output_line: usize, before_cursor: &str) {
        if self.program_reads {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(c) if !ctrl && !alt => {
                self.typing_starts(output_line, before_cursor);
                line.insert_char(self.shown(c));
            }
            // Editing the line, as when nothing runs -- reported: what was
            // typed couldn't be deleted. Earlier text too: it's the
            // user's line.
            KeyCode::Backspace | KeyCode::Delete => {
                line.apply_key(key);
                self.start = self.start.min(line.text().chars().count());
            }
            KeyCode::Esc => {
                line.clear();
                self.start = 0;
                self.echo_from = None;
                self.before_typing = None;
            }
            KeyCode::Enter => self.drop_typed(line),
            KeyCode::Char('c' | 'C') if ctrl => self.drop_typed(line),
            _ => {}
        }
    }

    /// Mirrors pasted text: lines ending in a break were sent; the rest
    /// is being typed.
    pub fn paste(&mut self, line: &mut TextField, text: &str, output_line: usize, before_cursor: &str) {
        if self.program_reads {
            return;
        }
        let text = text.replace('\r', "");
        let typed = match text.rsplit_once('\n') {
            Some((_, rest)) => {
                self.drop_typed(line);
                rest
            }
            None => text.as_str(),
        };
        for c in typed.chars() {
            self.typing_starts(output_line, before_cursor);
            line.insert_char(self.shown(c));
        }
    }

    /// New output: if the typed text now sits right before the program's
    /// cursor, where it wasn't when typed, the program echoed it -- it
    /// reads its input. The text leaves our line, and so will the rest.
    pub fn observe(&mut self, line: &mut TextField, before_cursor: &str) {
        let typed: String = line.text().chars().skip(self.start).collect();
        if self.program_reads || typed.is_empty() || self.before_typing.as_deref() == Some(before_cursor) {
            return;
        }
        if before_cursor.ends_with(&typed) {
            self.drop_typed(line);
            self.program_reads = true;
        }
    }

    fn typing_starts(&mut self, output_line: usize, before_cursor: &str) {
        if self.echo_from.is_none() {
            self.echo_from = Some(output_line);
            self.before_typing = Some(before_cursor.to_string());
            self.secret = asks_for_secret(before_cursor);
        }
    }

    /// What our line shows for a typed `c`.
    fn shown(&self, c: char) -> char {
        if self.secret {
            '*'
        } else {
            c
        }
    }

    /// The program ended with `output`: the typed text stays in `line`
    /// unless it shows up in the output since it was typed.
    pub fn finish(mut self, line: &mut TextField, output: &[Line]) {
        let typed: String = line.text().chars().skip(self.start).collect();
        if typed.is_empty() {
            return;
        }
        // Not a command for the next prompt: an unsent password.
        if self.secret {
            self.drop_typed(line);
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
        self.before_typing = None;
        self.secret = false;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    fn type_text(type_ahead: &mut TypeAhead, line: &mut TextField, text: &str, output_line: usize) {
        for c in text.chars() {
            type_ahead.key(line, key(KeyCode::Char(c)), output_line, "");
        }
    }

    /// Reported: the next command typed during `dir` was lost.
    #[test]
    fn text_the_program_never_read_stays_in_the_command_line() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text(&mut type_ahead, &mut line, "svn sx", 3);
        type_ahead.key(&mut line, key(KeyCode::Backspace), 3, "");
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
        type_ahead.key(&mut line, key(KeyCode::Enter), 0, "");
        type_text(&mut type_ahead, &mut line, "next", 1);

        type_ahead.finish(&mut line, &[Line::raw("Password:"), Line::raw("ok")]);

        assert_eq!(line.text(), "next");
    }

    #[test]
    fn earlier_text_stays_through_a_sent_line_and_ctrl_c() {
        let mut line = TextField::with_text("cd ");
        let mut type_ahead = TypeAhead::begin(&mut line);

        type_ahead.paste(&mut line, "first\r\nsrc", 0, "");
        assert_eq!(line.text(), "cd src", "a pasted line ending in a break was sent");

        type_ahead.key(&mut line, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), 0, "");
        assert_eq!(line.text(), "cd ", "Ctrl+C drops the typed text");
    }

    /// Reported: a paste into a script's prompt showed twice, at the
    /// prompt and in our command line.
    #[test]
    fn once_the_program_echoes_the_typing_it_gets_the_keys_alone() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_ahead.key(&mut line, key(KeyCode::Char('a')), 0, "Name: ");
        type_ahead.observe(&mut line, "Name: ");
        assert_eq!(line.text(), "a", "not echoed yet");

        type_ahead.observe(&mut line, "Name: a");
        assert_eq!(line.text(), "", "echoed: the program reads");
        type_ahead.paste(&mut line, "bcd", 0, "Name: a");
        type_ahead.key(&mut line, key(KeyCode::Char('e')), 0, "Name: abcd");
        assert_eq!(line.text(), "", "nothing mirrored any more");
    }

    #[test]
    fn output_that_merely_ends_like_the_typing_isnt_an_echo() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_ahead.key(&mut line, key(KeyCode::Char('o')), 0, "Compiling foo");

        type_ahead.observe(&mut line, "Compiling foo");

        assert_eq!(line.text(), "o", "the line already ended with it when typed");
    }

    /// Reported: what was typed during a run couldn't be deleted.
    #[test]
    fn the_line_can_be_edited_while_a_command_runs() {
        let mut line = TextField::with_text("svn ");
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text(&mut type_ahead, &mut line, "stx", 0);

        type_ahead.key(&mut line, key(KeyCode::Backspace), 0, "");
        assert_eq!(line.text(), "svn st");
        for _ in 0..3 {
            type_ahead.key(&mut line, key(KeyCode::Backspace), 0, "");
        }
        assert_eq!(line.text(), "svn", "what was there before too");

        type_ahead.key(&mut line, key(KeyCode::Esc), 0, "");
        assert_eq!(line.text(), "", "Esc clears the line");
        type_text(&mut type_ahead, &mut line, "dir", 0);
        type_ahead.finish(&mut line, &[Line::raw("output")]);
        assert_eq!(line.text(), "dir");
    }

    #[test]
    fn password_prompts_are_told_by_their_words() {
        for prompt in ["password: ", "Password for 'https://hg.example.com':", "Enter passphrase for key 'C:/u/.ssh/id_rsa': ", "me@host's password:", "Пароль:", "PIN:", "Passwort für 'https://git.example.com':", "Enter password:"] {
            assert!(asks_for_secret(prompt), "{prompt:?}");
        }
        for prompt in ["C:\\work> ", "Proceed? [y/N]: ", "password reset done", "Enter your name:", "Shipping:"] {
            assert!(!asks_for_secret(prompt), "{prompt:?}");
        }
    }

    /// Reported: `hg pull`'s password showed in the clear in our line.
    #[test]
    fn a_password_shows_as_stars_and_never_stays() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        for c in "s3cr".chars() {
            type_ahead.key(&mut line, key(KeyCode::Char(c)), 2, "password: ");
        }
        type_ahead.paste(&mut line, "et!", 2, "password: ");
        assert_eq!(line.text(), "*******");

        type_ahead.finish(&mut line, &[Line::raw("password: ")]);
        assert_eq!(line.text(), "", "an unsent password isn't left for the next prompt");
    }

    /// After the password's line is sent, the next text is shown as typed.
    #[test]
    fn after_the_password_the_next_text_shows_as_typed() {
        let mut line = TextField::new();
        let mut type_ahead = TypeAhead::begin(&mut line);
        type_text_at(&mut type_ahead, &mut line, "pw", "password: ");
        type_ahead.key(&mut line, key(KeyCode::Enter), 2, "password: ");
        type_text_at(&mut type_ahead, &mut line, "hg up", "pulling...");

        assert_eq!(line.text(), "hg up");
    }

    fn type_text_at(type_ahead: &mut TypeAhead, line: &mut TextField, text: &str, before_cursor: &str) {
        for c in text.chars() {
            type_ahead.key(line, key(KeyCode::Char(c)), 2, before_cursor);
        }
    }
}
