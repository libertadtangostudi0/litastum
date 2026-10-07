use std::io::Stdout;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::App;
use crate::ui;
use crate::user_screen::{encode_key, LiveCommand, LiveView};

use super::shell_exec::{resolve_app_paths_command, shell_argument};
use super::type_ahead::TypeAhead;

/// How long to wait for a key between looks at the program's output.
const FRAME: Duration = Duration::from_millis(16);


/// Runs `line` through the active shell profile in a pseudoconsole, in the
/// active panel's directory, showing the user screen with the output live
/// until the program exits; keys go to the program (`Ctrl+C` included),
/// and what's typed shows in the command line too (`TypeAhead`). Its
/// output then stays on the user screen. A failed launch is said there.
pub(super) fn run_live(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, line: &str) -> Result<()> {
    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    let mut args = profile.args_prefix.clone();
    args.push(shell_argument(&resolve_app_paths_command(line)));
    let size = terminal.size()?;
    let mut rows = ui::console_rows(size.height);
    debug!(shell = profile.name, %line, cwd = %cwd.display(), "running a command in a pseudoconsole");
    let command = match LiveCommand::spawn(&profile.program, args, &cwd, size.width, rows) {
        Ok(command) => command,
        Err(err) => {
            let theme = app.theme;
            app.user_screen.push_message(&format!("failed to launch '{}': {err}", profile.program), &theme);
            return Ok(());
        }
    };

    let mut type_ahead = TypeAhead::begin(&mut app.command_line);
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
        if !event::poll(FRAME)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let key = crate::keyboard_layout::normalize_ctrl_shortcut(key);
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
                if !command.full_screen() {
                    type_ahead.paste(&mut app.command_line, &text, command.output_line(), &command.text_before_cursor());
                    redraw = true;
                }
                command.write(text.into_bytes());
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


/// One frame of the user screen (`ui::draw_console`), then its cursor.
pub(super) fn draw_console(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &App, live: Option<&LiveView>) -> Result<()> {
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
