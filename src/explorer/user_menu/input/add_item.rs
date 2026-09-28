use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Overlay};

/// Key handling on the `Ins` add-item form (`Overlay::AddUserMenuItem`):
/// `Esc` cancels back to browsing the menu untouched at any stage;
/// `Enter` on the title field advances to the command field (a no-op
/// on a blank title); `Enter` on the command field inserts the
/// finished item (`UserMenuState::insert_item`, which also persists)
/// and returns to browsing with it selected. Anything else edits
/// whichever field is currently active.
pub fn handle_add_user_menu_item_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        let Some(Overlay::AddUserMenuItem(menu, _)) = app.overlay.take() else {
            return Ok(());
        };
        app.overlay = Some(Overlay::UserMenu(menu));
        return Ok(());
    }

    if key.code == KeyCode::Enter {
        let Some(Overlay::AddUserMenuItem(_, form)) = &mut app.overlay else {
            return Ok(());
        };
        if form.is_title_stage() {
            form.advance_from_title();
            return Ok(());
        }

        let Some(Overlay::AddUserMenuItem(mut menu, form)) = app.overlay.take() else {
            unreachable!("just matched above");
        };
        menu.insert_item(form.finish());
        app.overlay = Some(Overlay::UserMenu(menu));
        return Ok(());
    }

    let Some(Overlay::AddUserMenuItem(_, form)) = &mut app.overlay else {
        return Ok(());
    };
    let field = if form.is_title_stage() { &mut form.title } else { &mut form.command };
    field.apply_key(key);

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Mode;
    use crate::explorer::user_menu::parse::{self, MenuItemBody};
    use crate::explorer::user_menu::input::scratch_dir;
    use crate::explorer::user_menu::state::{AddUserMenuItemState, UserMenuState};
    use crate::test_support::{key, test_app};

    fn app_in_add_form(existing_content: &str) -> App {
        let dir = scratch_dir();
        let menu = UserMenuState::from_items(dir.clone(), parse::parse(existing_content));
        let mut app = test_app(dir);
        app.overlay = Some(Overlay::AddUserMenuItem(menu, AddUserMenuItemState::new()));
        app
    }

    #[test]
    fn typing_edits_the_title_field() {
        let mut app = app_in_add_form("a: A\necho a\n");

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Some(Overlay::AddUserMenuItem(_, form)) = &app.overlay else { panic!("expected Overlay::AddUserMenuItem") };
        assert_eq!(form.title.text(), "x");
    }

    #[test]
    fn enter_on_a_blank_title_does_not_advance() {
        let mut app = app_in_add_form("a: A\necho a\n");

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::AddUserMenuItem(_, form)) = &app.overlay else { panic!("expected Overlay::AddUserMenuItem") };
        assert!(form.is_title_stage());
    }

    #[test]
    fn enter_advances_to_the_command_stage_then_typing_edits_it() {
        let mut app = app_in_add_form("a: A\necho a\n");
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap();

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('c'))).unwrap();

        let Some(Overlay::AddUserMenuItem(_, form)) = &app.overlay else { panic!("expected Overlay::AddUserMenuItem") };
        assert!(!form.is_title_stage());
        assert_eq!(form.command.text(), "c");
    }

    /// The actual end-to-end point of the whole feature: a leaf
    /// item typed through the form ends up in the menu, selected,
    /// and persisted to `LitastumMenu.toml`.
    #[test]
    fn enter_on_the_command_stage_inserts_the_item_and_returns_to_browsing() {
        let dir = scratch_dir();
        let mut app = app_in_add_form_at(&dir, "a: A\necho a\n");
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap(); // -> command stage
        for c in "echo hi".chars() {
            handle_add_user_menu_item_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::UserMenu(menu)) = &app.overlay else { panic!("expected Overlay::UserMenu") };
        let titles: Vec<&str> = menu.current_level().items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["A", "n"], "the new item should be inserted right after the selected one");
        assert!(dir.join("LitastumMenu.toml").is_file(), "should have persisted the change");
    }

    /// A blank command builds a submenu instead of a leaf item.
    #[test]
    fn enter_on_a_blank_command_inserts_an_empty_submenu() {
        let mut app = app_in_add_form("a: A\necho a\n");
        for c in "git".chars() {
            handle_add_user_menu_item_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap(); // -> command stage, left blank

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::UserMenu(menu)) = &app.overlay else { panic!("expected Overlay::UserMenu") };
        let new_item = menu.current_level().items.iter().find(|i| i.title == "git").unwrap();
        assert_eq!(new_item.body, MenuItemBody::Submenu(Vec::new()));
    }

    #[test]
    fn esc_cancels_back_to_the_menu_unchanged() {
        let mut app = app_in_add_form("a: A\necho a\n");
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Some(Overlay::UserMenu(menu)) = &app.overlay else { panic!("expected Overlay::UserMenu") };
        assert_eq!(menu.current_level().items.len(), 1, "nothing should have been added");
    }

    #[test]
    fn is_a_noop_outside_add_item_mode() {
        let mut app = app_in_add_form("a: A\necho a\n");
        app.overlay = None;

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }

    /// `app_in_add_form` above always builds a fresh scratch dir;
    /// this variant takes an existing one, needed by the one test
    /// that checks the persisted file afterward.
    fn app_in_add_form_at(dir: &std::path::Path, existing_content: &str) -> App {
        let menu = UserMenuState::from_items(dir.to_path_buf(), parse::parse(existing_content));
        let mut app = test_app(dir.to_path_buf());
        app.overlay = Some(Overlay::AddUserMenuItem(menu, AddUserMenuItemState::new()));
        app
    }
}
