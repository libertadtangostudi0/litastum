use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;

use crate::app::{App, Overlay};
use crate::explorer::user_menu::state::{self, UserMenuState};
use crate::yes_no::{self, Answer};

/// Key handling on `Overlay::ConfirmPortFarMenu` -- opened either by `F2`
/// (`explorer::command::open_user_menu`) or by the startup check
/// (`main.rs`) finding a `FarMenu.ini`, regardless of whether
/// `LitastumMenu.toml` already exists (`state::resolve_menu`'s own doc
/// comment). `Y` actually ports it (`state::port_far_menu` -- parses
/// the DSL, backs up and overwrites `LitastumMenu.toml`, moves
/// `FarMenu.ini` aside to `FarMenu.ini.bak`) and opens the result for
/// browsing; `N`/`Esc` still moves `FarMenu.ini` aside
/// (`state::backup_far_menu_without_porting`, same backup name, just
/// without reading it) so it stops being re-detected, and shows a
/// one-line `Overlay::Info` telling the user where it ended up.
pub fn handle_confirm_port_far_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(Overlay::ConfirmPortFarMenu(_)) = &app.overlay else {
        return Ok(());
    };

    match yes_no::answer(key) {
        Answer::Yes => {
            let Some(Overlay::ConfirmPortFarMenu(far_path)) = app.overlay.take() else {
                unreachable!("just matched Overlay::ConfirmPortFarMenu above");
            };
            let items = state::port_far_menu(&far_path);
            let dir = far_path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            app.overlay = Some(Overlay::UserMenu(UserMenuState::from_items(dir, items)));
        }
        Answer::No => {
            let Some(Overlay::ConfirmPortFarMenu(far_path)) = app.overlay.take() else {
                unreachable!("just matched Overlay::ConfirmPortFarMenu above");
            };
            if let Some(backup) = state::backup_far_menu_without_porting(&far_path) {
                app.overlay = Some(Overlay::Info(format!("FarMenu.ini backed up as {}", backup.display())));
            }
        }
        Answer::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::KeyCode;

    use super::*;
    use crate::app::Mode;
    use crate::explorer::user_menu::input::scratch_dir;
    use crate::test_support::{key, test_app};

    fn app_with_far_menu(content: &str) -> (App, std::path::PathBuf) {
        let dir = scratch_dir();
        let far_path = dir.join("FarMenu.ini");
        fs::write(&far_path, content).unwrap();
        let mut app = test_app(dir);
        app.overlay = Some(Overlay::ConfirmPortFarMenu(far_path.clone()));
        (app, far_path)
    }

    #[test]
    fn y_ports_the_file_and_opens_the_menu() {
        let (mut app, far_path) = app_with_far_menu("s: status\ngit status -s\n");

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        let Some(Overlay::UserMenu(menu)) = &app.overlay else { panic!("expected Overlay::UserMenu") };
        assert_eq!(menu.current_level().items[0].title, "status");
        assert!(far_path.with_file_name("LitastumMenu.toml").is_file(), "should have written LitastumMenu.toml alongside FarMenu.ini");
    }

    /// Regression coverage for the real request: declining still
    /// has to move `FarMenu.ini` out of the way (so it isn't
    /// re-detected forever) and tell the user where it went --
    /// `Overlay::Info`, not a silent return to `Mode::Browsing`.
    #[test]
    fn n_backs_up_far_menu_without_porting_and_shows_where() {
        let (mut app, far_path) = app_with_far_menu("s: status\ngit status -s\n");

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Char('n'))).unwrap();

        assert!(!far_path.with_file_name("LitastumMenu.toml").exists(), "must not have ported anything");
        assert!(!far_path.exists(), "FarMenu.ini should have moved aside");
        assert!(far_path.with_file_name("FarMenu.ini.bak").is_file());
        let Some(Overlay::Info(message)) = &app.overlay else { panic!("expected Overlay::Info") };
        assert!(message.contains("FarMenu.ini.bak"), "message should say where it ended up: {message:?}");
    }

    #[test]
    fn esc_cancels_the_same_as_n() {
        let (mut app, far_path) = app_with_far_menu("s: status\ngit status -s\n");

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::Info(_))));
        assert!(far_path.with_file_name("FarMenu.ini.bak").is_file());
    }

    #[test]
    fn is_a_noop_outside_confirm_port_mode() {
        let (mut app, _far_path) = app_with_far_menu("s: status\ngit status -s\n");
        app.overlay = None;

        handle_confirm_port_far_menu_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }
}
