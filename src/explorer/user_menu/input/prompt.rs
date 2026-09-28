use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::{App, Mode};
use crate::command_line;

/// Key handling on a user-menu item's own `!?Label?Default!` prompt
/// popup (`Mode::UserMenuPrompt`) -- a standard single-line text field
/// (`TextField::apply_key`), one prompt at a time. `Enter` accepts the
/// current field's value and either advances to the next prompt or, if
/// that was the last one, runs the finished, fully-substituted command
/// list (same `run_shell_command_lines` path `handle_user_menu_key`
/// uses for a prompt-less item) and returns to browsing. `Esc` cancels
/// the whole item -- no partial run of some-but-not-all commands.
pub fn handle_user_menu_prompt_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    if key.code == KeyCode::Esc {
        app.mode = Mode::Browsing;
        return Ok(());
    }
    if key.code == KeyCode::Enter {
        let Mode::UserMenuPrompt(prompt) = &mut app.mode else {
            return Ok(());
        };
        if let Some(commands) = prompt.accept_current() {
            app.mode = Mode::Browsing;
            return command_line::run_shell_command_lines(app, terminal, &commands);
        }
        return Ok(());
    }

    let Mode::UserMenuPrompt(prompt) = &mut app.mode else {
        return Ok(());
    };
    prompt.value.apply_key(key);

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::user_menu::input::{dummy_terminal, scratch_dir};
    use crate::explorer::user_menu::parse::Prompt;
    use crate::explorer::user_menu::state::UserMenuPromptState;
    use crate::test_support::{key, test_app};

    fn app_in_prompt() -> App {
        let prompts = vec![Prompt { label: "Name".to_string(), default: String::new() }];
        let mut app = test_app(scratch_dir());
        app.mode = Mode::UserMenuPrompt(UserMenuPromptState::new(vec!["echo !?Name?!".to_string()], prompts));
        app
    }

    #[test]
    fn typing_edits_the_field_in_place() {
        let mut app = app_in_prompt();
        let mut terminal = dummy_terminal();

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x')), &mut terminal).unwrap();

        let Mode::UserMenuPrompt(prompt) = &app.mode else { panic!("expected Mode::UserMenuPrompt") };
        assert_eq!(prompt.value.text(), "x");
    }

    #[test]
    fn esc_cancels_back_to_browsing() {
        let mut app = app_in_prompt();
        let mut terminal = dummy_terminal();

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Esc), &mut terminal).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn is_a_noop_outside_prompt_mode() {
        let mut app = app_in_prompt();
        app.mode = Mode::Browsing;
        let mut terminal = dummy_terminal();

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x')), &mut terminal).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
