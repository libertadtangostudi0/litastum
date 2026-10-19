use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyEventKind, MouseButton, MouseEventKind};
use tracing::debug;

use crate::app::App;
use crate::ui;
use crate::user_screen::{encode_key, LiveCommand, LiveView};
use crate::terminal_setup::Tui;

use super::shell_exec::{resolve_app_paths_command, shell_argument};
use super::type_ahead::TypeAhead;

/// How long to wait for a key between looks at the program's output.
const FRAME: Duration = Duration::from_millis(16);


/// Runs `line` through the active shell profile in a pseudoconsole, in the
/// active panel's directory, showing the user screen with the output live
/// until the program exits; keys go to the program (`Ctrl+C` included),
/// and what's typed shows in the command line too (`TypeAhead`). Its
/// output then stays on the user screen. A failed launch is said there.
pub(super) fn run_live(app: &mut App, terminal: &mut Tui, line: &str) -> Result<()> {
    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    let mut args = profile.args_prefix.clone();
    args.push(shell_argument(&resolve_app_paths_command(line)));
    let size = terminal.size()?;
    let mut rows = ui::console_rows(size.height);
    let shown = crate::command_line::secrets::redact(line);
    debug!(shell = profile.name, line = %shown, cwd = %cwd.display(), "running a command in a pseudoconsole");
    let command = match LiveCommand::spawn(&profile.program, args, &cwd, size.width, rows) {
        Ok(command) => command,
        Err(err) => {
            let theme = app.theme;
            app.user_screen.push_message(&format!("failed to launch '{}': {err}", profile.program), &theme);
            return Ok(());
        }
    };

    let mut type_ahead = TypeAhead::begin(&mut app.command_line);
    // A paste of several lines, waiting for `Enter` (`needs_confirmation`).
    let mut waiting: Option<String> = None;
    let mut redraw = true;
    loop {
        if command.take_changed() || redraw {
            type_ahead.observe(&mut app.command_line, &command.text_before_cursor());
            draw_console(terminal, app, Some(&command.view(usize::from(rows))))?;
            redraw = false;
        }
        if command.is_finished() {
            break;
        }
        app.clipboard_wipe.poll(std::time::Instant::now());
        // Windows Terminal keeps `Ctrl+V` and types the clipboard in as
        // keys -- a password pasted that way wasn't accepted (reported).
        // As in the main loop: the key is polled, the clipboard pasted
        // whole, the typed copy swallowed.
        #[cfg(windows)]
        if app.paste_flood.ctrl_v_just_pressed(app.terminal_focused) {
            if let Some(text) = crate::text_field::clipboard::get() {
                let rest = app.paste_flood.not_yet_delivered(&text);
                app.paste_flood.expect(&rest);
                request_paste(app, &command, &mut type_ahead, &mut waiting, rest);
                redraw = true;
            }
        }
        if !event::poll(FRAME)? {
            continue;
        }
        let event = event::read()?;
        if app.note_focus(&event) {
            continue;
        }
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let key = crate::keyboard_layout::normalize_ctrl_shortcut(key);
                if app.paste_flood.should_swallow(key.code, key.modifiers) || (cfg!(windows) && is_ctrl_v(key)) {
                    continue;
                }
                // The answer to a waiting paste: `Enter` pastes, `Esc`
                // drops it, any other key drops it and does its job.
                if let Some(text) = waiting.take() {
                    app.notice = None;
                    redraw = true;
                    match key.code {
                        crossterm::event::KeyCode::Enter => {
                            paste_into(app, &command, &mut type_ahead, &text);
                            continue;
                        }
                        crossterm::event::KeyCode::Esc => continue,
                        _ => {}
                    }
                }
                app.paste_flood.record_typed_key(key.code, key.modifiers);
                if let Some(bytes) = encode_key(key, command.app_cursor()) {
                    command.write(bytes);
                }
                // A full-screen program's keys are commands, not text.
                if !command.full_screen() {
                    type_ahead.key(&mut app.command_line, key, command.output_line(), &command.text_before_cursor());
                    redraw = true;
                }
            }
            Event::Paste(text) => {
                request_paste(app, &command, &mut type_ahead, &mut waiting, text);
                redraw = true;
            }
            // The right button pastes, as in a console window (requested).
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Right) => {
                if let Some(text) = crate::text_field::clipboard::get() {
                    request_paste(app, &command, &mut type_ahead, &mut waiting, text);
                    redraw = true;
                }
            }
            Event::Resize(width, height) => {
                rows = ui::console_rows(height);
                command.resize(width, rows);
                redraw = true;
            }
            _ => {}
        }
    }
    let output = command.output();
    type_ahead.finish(&mut app.command_line, &output);
    app.user_screen.extend(output);
    Ok(())
}


/// A paste into the running program: at once, or -- several lines, each
/// run as it arrives -- after a question (`needs_confirmation`).
fn request_paste(app: &mut App, command: &LiveCommand, type_ahead: &mut TypeAhead, waiting: &mut Option<String>, text: String) {
    if needs_confirmation(&text, &command.text_before_cursor()) {
        let lines = text.lines().count();
        app.notice = Some(crate::notice::Notice::info(format!("Paste {lines} lines into the program? Enter: paste, Esc: cancel")));
        *waiting = Some(text);
    } else {
        paste_into(app, command, type_ahead, &text);
    }
}


/// Whether a paste asks first (requested): several lines go to the
/// program each with `Enter`, run one by one -- text copied from a page
/// can hide a command. Not one line with a break at its end (a copied
/// command), nor at a password prompt.
fn needs_confirmation(text: &str, before_cursor: &str) -> bool {
    let body = text.trim_end_matches(['\r', '\n']);
    body.contains(['\r', '\n']) && !super::type_ahead::asks_for_secret(before_cursor)
}


/// Pastes `text` into the running program, and mirrors it in our line
/// (`TypeAhead`, as `*`s at a password prompt). Line breaks go as `Enter`
/// presses do (`\r`). Never logged: it may be a password.
fn paste_into(app: &mut App, command: &LiveCommand, type_ahead: &mut TypeAhead, text: &str) {
    if text.is_empty() {
        return;
    }
    let before_cursor = command.text_before_cursor();
    if !command.full_screen() {
        type_ahead.paste(&mut app.command_line, text, command.output_line(), &before_cursor);
    }
    // A password: off the clipboard in a while (`clipboard_wipe`).
    if super::type_ahead::asks_for_secret(&before_cursor) {
        app.clipboard_wipe.arm(text, std::time::Instant::now());
    }
    command.paste(&program_text(text));
}


/// `text` as a terminal types it: each line break an `Enter` (`\r`).
fn program_text(text: &str) -> String {
    text.replace("\r\n", "\r").replace('\n', "\r")
}


/// `Ctrl+V` as a key, when the terminal hands it over rather than pasting
/// itself: on Windows the paste is ours (the key is polled), and the key
/// must not reach the program as `0x16` ahead of the pasted text.
fn is_ctrl_v(key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    matches!(key.code, KeyCode::Char('v' | 'V')) && key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT)
}


/// One frame of the user screen (`ui::draw_console`), then its cursor.
pub(super) fn draw_console(terminal: &mut Tui, app: &App, live: Option<&LiveView>) -> Result<()> {
    let mut cursor = None;
    terminal.draw(|frame| cursor = ui::draw_console(frame, app, live))?;
    match cursor {
        Some(position) => {
            terminal.set_cursor_position(position)?;
            terminal.show_cursor()?;
        }
        None => terminal.hide_cursor()?,
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Requested: several lines ask first; one, or a password, don't.
    #[test]
    fn only_a_paste_of_several_lines_asks_first() {
        assert!(needs_confirmation("dir\r\ndel *.*\r\n", "C:\\> "));
        assert!(needs_confirmation("a\nb", ""));
        assert!(!needs_confirmation("hg pull\r\n", "C:\\> "), "one copied command");
        assert!(!needs_confirmation("plain", ""));
        assert!(!needs_confirmation("pw\nmore", "password: "), "at a password prompt");
    }

    #[test]
    fn ctrl_v_is_the_paste_not_a_key_for_the_program() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        assert!(is_ctrl_v(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)));
        assert!(!is_ctrl_v(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)), "a plain v is typed");
        assert!(!is_ctrl_v(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL | KeyModifiers::ALT)), "AltGr+v is a character");
    }

    #[test]
    fn a_paste_goes_to_the_program_with_enter_for_each_line_break() {
        assert_eq!(program_text("pass"), "pass", "a password as it is");
        assert_eq!(program_text("a\r\nb\nc"), "a\rb\rc");
    }
}
