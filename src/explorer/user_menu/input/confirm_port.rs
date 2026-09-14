use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Mode};
use crate::explorer::user_menu::state::{self, UserMenuState};

/// A user-triggered answer on the `Mode::ConfirmPortFarMenu` prompt --
/// same shape as `explorer::keymap::ConfirmDeleteCommand`, kept local
/// to this module rather than shared with it since the two prompts
/// don't otherwise have anything in common.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortFarMenuCommand {
    Confirm,
    Cancel,
    Ignore,
}

/// Resolves a raw key press on the "port FarMenu.ini?" prompt -- `Y`
/// confirms, `N`/`Esc` cancels, same convention as
/// `explorer::keymap::resolve_confirm_delete`.
pub fn resolve_port_far_menu(key: KeyEvent) -> PortFarMenuCommand {
    match key.code {
        KeyCode::Char('y' | 'Y') => PortFarMenuCommand::Confirm,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => PortFarMenuCommand::Cancel,
        _ => PortFarMenuCommand::Ignore,
    }
}

/// Key handling on `Mode::ConfirmPortFarMenu` -- opened either by `F2`
/// (`explorer::command::open_user_menu`) or by the startup check
/// (`main.rs`) finding a `FarMenu.ini`, regardless of whether
/// `LitastumMenu.toml` already exists (`state::resolve_menu`'s own doc
/// comment). `Y` actually ports it (`state::port_far_menu` -- parses
/// the DSL, backs up and overwrites `LitastumMenu.toml`, moves
/// `FarMenu.ini` aside to `FarMenu.ini.bak`) and opens the result for
/// browsing; `N`/`Esc` still moves `FarMenu.ini` aside
/// (`state::backup_far_menu_without_porting`, same backup name, just
/// without reading it) so it stops being re-detected, and shows a
/// one-line `Mode::Info` telling the user where it ended up.
pub fn handle_confirm_port_far_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ConfirmPortFarMenu(_) = &app.mode else {
        return Ok(());
    };

    match resolve_port_far_menu(key) {
        PortFarMenuCommand::Confirm => {
            let Mode::ConfirmPortFarMenu(far_path) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::ConfirmPortFarMenu above");
            };
            let items = state::port_far_menu(&far_path);
            let dir = far_path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            app.mode = Mode::UserMenu(UserMenuState::from_items(dir, items));
        }
        PortFarMenuCommand::Cancel => {
            let Mode::ConfirmPortFarMenu(far_path) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::ConfirmPortFarMenu above");
            };
            if let Some(backup) = state::backup_far_menu_without_porting(&far_path) {
                app.mode = Mode::Info(format!("FarMenu.ini backed up as {}", backup.display()));
            }
        }
        PortFarMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::explorer::user_menu::input::scratch_dir;
    use crate::test_support::{key, test_app};

    fn app_with_far_menu(content: &str) -> (App, std::path::PathBuf) {
        let dir = scratch_dir();
        let far_path = dir.join("FarMenu.ini");
        fs::write(&far_path, content).unwrap();
        let mut app = test_app(dir);
        app.mode = Mode::ConfirmPortFarMenu(far_path.clone());
        (app, far_path)
    }

    #[test]
    fn y_ports_the_file_and_opens_the_menu() {
        let (mut app, far_path) = app_with_far_menu("s: status\ngit status -s\n");

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
        assert_eq!(menu.current_level().items[0].title, "status");
        assert!(far_path.with_file_name("LitastumMenu.toml").is_file(), "should have written LitastumMenu.toml alongside FarMenu.ini");
    }

    /// Regression coverage for the real request: declining still
    /// has to move `FarMenu.ini` out of the way (so it isn't
    /// re-detected forever) and tell the user where it went --
    /// `Mode::Info`, not a silent return to `Mode::Browsing`.
    #[test]
    fn n_backs_up_far_menu_without_porting_and_shows_where() {
        let (mut app, far_path) = app_with_far_menu("s: status\ngit status -s\n");

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Char('n'))).unwrap();

        assert!(!far_path.with_file_name("LitastumMenu.toml").exists(), "must not have ported anything");
        assert!(!far_path.exists(), "FarMenu.ini should have moved aside");
        assert!(far_path.with_file_name("FarMenu.ini.bak").is_file());
        let Mode::Info(message) = &app.mode else { panic!("expected Mode::Info") };
        assert!(message.contains("FarMenu.ini.bak"), "message should say where it ended up: {message:?}");
    }

    #[test]
    fn esc_cancels_the_same_as_n() {
        let (mut app, far_path) = app_with_far_menu("s: status\ngit status -s\n");

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Info(_)));
        assert!(far_path.with_file_name("FarMenu.ini.bak").is_file());
    }

    #[test]
    fn is_a_noop_outside_confirm_port_mode() {
        let (mut app, _far_path) = app_with_far_menu("s: status\ngit status -s\n");
        app.mode = Mode::Browsing;

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
