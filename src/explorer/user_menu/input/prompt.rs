use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Mode};
use crate::command_line::Effect;

/// Key handling on a user-menu item's own `!?Label?Default!` prompt
/// popup (`Mode::UserMenuPrompt`) -- a standard single-line text field
/// (`TextField::apply_key`), one prompt at a time. `Enter` accepts the
/// current field's value and either advances to the next prompt or, if
/// that was the last one, runs the finished, fully-substituted command
/// list (`Effect::RunShell`, same as `handle_user_menu_key` for a
/// prompt-less item) and returns to browsing. `Esc` cancels
/// the whole item -- no partial run of some-but-not-all commands.
pub fn handle_user_menu_prompt_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if key.code == KeyCode::Esc {
        app.mode = Mode::Browsing;
        return Ok(Effect::None);
    }
    if key.code == KeyCode::Enter {
        let Mode::UserMenuPrompt(prompt) = &mut app.mode else {
            return Ok(Effect::None);
        };
        if let Some(commands) = prompt.accept_current() {
            app.mode = Mode::Browsing;
            return Ok(Effect::RunShell(commands));
        }
        return Ok(Effect::None);
    }

    let Mode::UserMenuPrompt(prompt) = &mut app.mode else {
        return Ok(Effect::None);
    };
    prompt.value.apply_key(key);

    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::user_menu::input::scratch_dir;
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

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::UserMenuPrompt(prompt) = &app.mode else { panic!("expected Mode::UserMenuPrompt") };
        assert_eq!(prompt.value.text(), "x");
    }

    #[test]
    fn enter_on_the_last_prompt_runs_the_substituted_commands() {
        let mut app = app_in_prompt();
        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let effect = handle_user_menu_prompt_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(effect, Effect::RunShell(vec!["echo x".to_string()]));
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn esc_cancels_back_to_browsing() {
        let mut app = app_in_prompt();

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn is_a_noop_outside_prompt_mode() {
        let mut app = app_in_prompt();
        app.mode = Mode::Browsing;

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
