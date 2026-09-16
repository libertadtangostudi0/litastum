use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};


/// A shell the command line can run typed input through — Windows
/// Terminal-style "profile" (see `.claude/rules/litastum-theming.md`'s
/// sibling doc, `.claude/rules/litastum-stack.md`, for the reasoning:
/// only universally-preinstalled shells are built in, no PATH/registry
/// probing for Git Bash/WSL/pwsh yet).
#[derive(Debug, Clone)]
pub struct ShellProfile {
    /// Shown in the `Ctrl+P` picker and the command-line's right edge.
    pub name: String,
    pub program: String,
    /// Arguments before the typed command itself, e.g. `["/C"]` for
    /// `cmd`, or `["-NoLogo", "-Command"]` for PowerShell.
    pub args_prefix: Vec<String>,
}


impl ShellProfile {
    fn new(name: &str, program: &str, args_prefix: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            program: program.to_string(),
            args_prefix: args_prefix.iter().map(|arg| arg.to_string()).collect(),
        }
    }
}


/// The built-in profiles for this platform. Always non-empty; index
/// `0` is the default `App::active_shell` starts on.
pub fn builtin_profiles() -> Vec<ShellProfile> {
    profiles_for_platform()
}


#[cfg(windows)]
fn profiles_for_platform() -> Vec<ShellProfile> {
    vec![
        ShellProfile::new("Command Prompt", "cmd", &["/C"]),
        ShellProfile::new("PowerShell", "powershell", &["-NoLogo", "-Command"]),
    ]
}


#[cfg(not(windows))]
fn profiles_for_platform() -> Vec<ShellProfile> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    vec![ShellProfile::new("Shell", &shell, &["-c"])]
}


/// Key handling on the `Ctrl+P` shell picker: `Up`/`Down` to move,
/// `Enter` sets `app.active_shell` and closes, `Esc` cancels. Not
/// persisted to `config.json` — resets to the platform default each
/// run (see `.claude/rules/litastum-stack.md`). Moved here from
/// `main.rs` so this module owns both the profile list and the popup
/// that picks from it (`app::ShellMenu`, the "which row is
/// highlighted" state, still lives on `App` — it's simple enough not
/// to need its own file the way `MainMenu`/`ThemeMenu` do).
pub fn handle_shell_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ShellMenu(menu) = &mut app.mode else {
        return Ok(());
    };
    debug!(?key, selected = menu.selected, "shell menu key");

    match key.code {
        KeyCode::Up => crate::list_cursor::move_up(&mut menu.selected),
        KeyCode::Down => crate::list_cursor::move_down(&mut menu.selected, app.shell_profiles.len()),
        KeyCode::Enter => {
            app.active_shell = menu.selected;
            app.mode = Mode::Browsing;
        }
        KeyCode::Esc => app.mode = Mode::Browsing,
        _ => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ShellMenu;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    #[test]
    fn builtin_profiles_is_never_empty() {
        assert!(!builtin_profiles().is_empty());
    }

    /// A real `App` (no terminal needed) in `Mode::ShellMenu`, cursor on
    /// row 0 — `App::new` always seeds at least one built-in profile, so
    /// there's always something to navigate.
    fn app_in_shell_menu() -> App {
        let mut app = test_app(unique_scratch_dir("shell-menu"));
        app.mode = Mode::ShellMenu(ShellMenu { selected: 0 });
        app
    }

    #[test]
    fn handle_shell_menu_key_down_is_clamped_at_the_last_profile() {
        let mut app = app_in_shell_menu();
        let profile_count = app.shell_profiles.len();

        for _ in 0..profile_count + 2 {
            handle_shell_menu_key(&mut app, key(KeyCode::Down)).unwrap();
        }

        let Mode::ShellMenu(menu) = &app.mode else { panic!("expected Mode::ShellMenu") };
        assert_eq!(menu.selected, profile_count - 1);
    }

    #[test]
    fn handle_shell_menu_key_up_is_clamped_at_zero() {
        let mut app = app_in_shell_menu();

        handle_shell_menu_key(&mut app, key(KeyCode::Up)).unwrap();

        let Mode::ShellMenu(menu) = &app.mode else { panic!("expected Mode::ShellMenu") };
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn handle_shell_menu_key_enter_applies_the_selection_and_closes() {
        let mut app = app_in_shell_menu();
        if app.shell_profiles.len() > 1 {
            handle_shell_menu_key(&mut app, key(KeyCode::Down)).unwrap();
        }
        let Mode::ShellMenu(menu) = &app.mode else { unreachable!() };
        let expected = menu.selected;

        handle_shell_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.active_shell, expected);
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_shell_menu_key_esc_cancels_without_changing_active_shell() {
        let mut app = app_in_shell_menu();
        let original_shell = app.active_shell;

        handle_shell_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert_eq!(app.active_shell, original_shell);
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_shell_menu_key_is_a_noop_outside_shell_menu_mode() {
        let mut app = app_in_shell_menu();
        app.mode = Mode::Browsing;

        handle_shell_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
