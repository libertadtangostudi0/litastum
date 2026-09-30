use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Overlay};
use crate::command_line::Effect;

/// Keys on a `!?Label?Default!` prompt (`Overlay::UserMenuPrompt`), one
/// field at a time. `Enter` accepts and moves to the next prompt, or runs
/// the substituted commands after the last one; `Esc` cancels the whole
/// item -- never a partial run.
pub fn handle_user_menu_prompt_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if key.code == KeyCode::Esc {
        app.overlay = None;
        return Ok(Effect::None);
    }
    if key.code == KeyCode::Enter {
        let Some(Overlay::UserMenuPrompt(prompt)) = &mut app.overlay else {
            return Ok(Effect::None);
        };
        if let Some(commands) = prompt.accept_current() {
            app.overlay = None;
            return Ok(Effect::RunShell(commands));
        }
        return Ok(Effect::None);
    }

    let Some(Overlay::UserMenuPrompt(prompt)) = &mut app.overlay else {
        return Ok(Effect::None);
    };
    prompt.value.apply_key(key);

    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Mode;
    use crate::explorer::user_menu::input::scratch_dir;
    use crate::explorer::user_menu::parse::Prompt;
    use crate::explorer::user_menu::state::UserMenuPromptState;
    use crate::test_support::{key, test_app};

    fn app_in_prompt() -> App {
        let prompts = vec![Prompt { label: "Name".to_string(), default: String::new() }];
        let mut app = test_app(scratch_dir());
        app.overlay = Some(Overlay::UserMenuPrompt(UserMenuPromptState::new(vec!["echo !?Name?!".to_string()], prompts)));
        app
    }

    #[test]
    fn typing_edits_the_field_in_place() {
        let mut app = app_in_prompt();

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Some(Overlay::UserMenuPrompt(prompt)) = &app.overlay else { panic!("expected Overlay::UserMenuPrompt") };
        assert_eq!(prompt.value.text(), "x");
    }

    #[test]
    fn enter_on_the_last_prompt_runs_the_substituted_commands() {
        let mut app = app_in_prompt();
        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let effect = handle_user_menu_prompt_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(effect, Effect::RunShell(vec!["echo x".to_string()]));
        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn esc_cancels_back_to_browsing() {
        let mut app = app_in_prompt();

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn is_a_noop_outside_prompt_mode() {
        let mut app = app_in_prompt();
        app.overlay = None;

        handle_user_menu_prompt_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }
}
