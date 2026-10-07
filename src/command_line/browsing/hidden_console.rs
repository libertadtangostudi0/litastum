use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::App;
use crate::command_line::effect::Effect;
use crate::text_field::EditOutcome;
use crate::ui;

use super::bindings::BrowserAction;
use super::live_command::draw_console;
use super::shell_exec::{run_shell_command_lines, submit_command_line};

/// Lines the mouse wheel scrolls the user screen per tick.
const WHEEL_LINES: isize = 3;


/// Whether an already-normalized `key` is `Ctrl+O`.
pub(super) fn is_ctrl_o(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)
}


/// `Ctrl+O` -- real Far Manager's own "show/hide panels" toggle: the user
/// screen (what commands printed, `App::user_screen`) with our own
/// command line and suggestions under it (`ui::draw_console`).
/// Blocking: the main loop doesn't redraw until the panels come back.
///
/// It's a real command line, as in Far: `Enter` runs the line here, its
/// output joining the screen live, and stays; `PageUp`/`PageDown` and the
/// mouse wheel scroll back; only `Ctrl+O` returns. History:
/// docs/history/command-execution.md.
pub(in crate::command_line) fn toggle_panels_hidden(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    loop {
        draw_console(terminal, app, None)?;
        let rows = usize::from(ui::console_rows(terminal.size()?.height));
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let key = crate::keyboard_layout::normalize_ctrl_shortcut(key);
                if is_ctrl_o(key) {
                    break;
                }
                if let Some(lines) = console_key(app, key, rows)? {
                    run_shell_command_lines(app, terminal, &lines)?;
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => app.user_screen.scroll_by(WHEEL_LINES, rows),
                MouseEventKind::ScrollDown => app.user_screen.scroll_by(-WHEEL_LINES, rows),
                _ => {}
            },
            Event::Paste(text) => {
                for c in text.chars().filter(|&c| c != '\n' && c != '\r') {
                    app.command_line.insert_char(c);
                }
                super::line_edited(app);
            }
            _ => {}
        }
    }
    app.active_panel().reload()?;
    Ok(())
}


/// A key on the user screen, `Ctrl+O` aside. While suggestions show,
/// `Up`/`Down`/`Tab`/`F8`/`Esc` work on them as over the panels; `Enter`
/// submits the line (returning what to run); `PageUp`/`PageDown` scroll;
/// `Esc` clears the line; `Tab` completes a path; the rest edits the line
/// -- arrows included, the panels being hidden.
fn console_key(app: &mut App, key: KeyEvent, rows: usize) -> Result<Option<Vec<String>>> {
    if super::suggestions_showing(app) {
        let action = match key.code {
            KeyCode::Up => Some(BrowserAction::SuggestionUp),
            KeyCode::Down => Some(BrowserAction::SuggestionDown),
            KeyCode::Tab => Some(BrowserAction::AcceptSuggestion),
            KeyCode::F(8) => Some(BrowserAction::DeleteSuggestion),
            KeyCode::Esc => {
                app.command_line_suggestion_dismissed = true;
                return Ok(None);
            }
            _ => None,
        };
        if let Some(action) = action {
            super::perform(app, action)?;
            return Ok(None);
        }
    }
    match key.code {
        KeyCode::Enter => {
            app.command_line_completion = None;
            match submit_command_line(app)? {
                Effect::RunShell(lines) => return Ok(Some(lines)),
                Effect::ClearScreen => app.user_screen.clear(),
                Effect::None | Effect::ToggleHiddenPanels => {}
            }
            super::line_edited(app);
        }
        KeyCode::PageUp => app.user_screen.scroll_by(rows as isize, rows),
        KeyCode::PageDown => app.user_screen.scroll_by(-(rows as isize), rows),
        KeyCode::Esc => {
            app.command_line.clear();
            super::line_edited(app);
        }
        KeyCode::Tab if !app.command_line.is_empty() => super::perform(app, BrowserAction::Complete).map(drop)?,
        _ => {
            if app.command_line.apply_key(key) == EditOutcome::TextChanged {
                super::line_edited(app);
            }
        }
    }
    Ok(None)
}


#[cfg(test)]
mod tests {
    use ratatui::text::Line;

    use super::*;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn typed(line: &str) -> App {
        let mut app = test_app(unique_scratch_dir("hidden-console"));
        app.command_line.set_text(line);
        app
    }

    #[test]
    fn matches_ctrl_o_regardless_of_other_held_modifiers() {
        assert!(is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)));
        assert!(is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)));
        assert!(!is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)));
        assert!(!is_ctrl_o(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn enter_hands_the_typed_line_over_to_run() {
        let mut app = typed("svn st");

        assert_eq!(console_key(&mut app, key(KeyCode::Enter), 10).unwrap(), Some(vec!["svn st".to_string()]));
        assert!(app.command_line.is_empty());
        assert_eq!(app.command_history, ["svn st"]);
    }

    #[test]
    fn cls_empties_the_user_screen_without_running_anything() {
        let mut app = typed("cls");
        app.user_screen.extend(vec![Line::raw("old output")]);

        assert_eq!(console_key(&mut app, key(KeyCode::Enter), 10).unwrap(), None);
        assert!(app.user_screen.lines().is_empty());
    }

    /// Requested: the suggestions work here as over the panels.
    #[test]
    fn suggestions_take_up_down_and_tab_and_esc_closes_them_first() {
        let mut app = typed("svn");
        app.command_history = vec!["svn up".into(), "svn st".into()];

        console_key(&mut app, key(KeyCode::Down), 10).unwrap();
        console_key(&mut app, key(KeyCode::Tab), 10).unwrap();
        assert_eq!(app.command_line.text(), "svn up", "newest first: svn st, then svn up");

        let mut app = typed("svn");
        app.command_history = vec!["svn up".into()];
        console_key(&mut app, key(KeyCode::Esc), 10).unwrap();
        assert_eq!(app.command_line.text(), "svn", "the first Esc closes the list");
        console_key(&mut app, key(KeyCode::Esc), 10).unwrap();
        assert!(app.command_line.is_empty(), "the next clears the line");
    }

    #[test]
    fn arrows_edit_the_line_and_page_keys_scroll() {
        let mut app = typed("ac");
        console_key(&mut app, key(KeyCode::Left), 10).unwrap();
        console_key(&mut app, key(KeyCode::Char('b')), 10).unwrap();
        assert_eq!(app.command_line.text(), "abc");

        app.user_screen.extend((0..30).map(|n| Line::raw(n.to_string())).collect());
        console_key(&mut app, key(KeyCode::PageUp), 10).unwrap();
        assert_eq!(app.user_screen.scroll(), 10);
        console_key(&mut app, key(KeyCode::PageDown), 10).unwrap();
        assert_eq!(app.user_screen.scroll(), 0);
    }
}
