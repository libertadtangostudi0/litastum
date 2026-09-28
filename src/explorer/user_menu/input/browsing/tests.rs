use std::fs;

use crossterm::event::KeyCode;

use super::*;
use crate::explorer::UserMenuState;
use crate::explorer::user_menu::input::{dummy_terminal, scratch_dir};
use crate::test_support::{key, test_app};

/// `content` is parsed as the DSL (`parse::parse`), same as
/// `FarMenu.ini`'s own format -- convenient shorthand for
/// building test fixtures by hand; not a claim that this is
/// what `LitastumMenu.toml` looks like on disk (see
/// `toml_format.rs` for that).
fn app_with_menu(content: &str) -> App {
    let dir = scratch_dir();
    let mut app = test_app(dir.clone());
    app.mode = Mode::UserMenu(UserMenuState::from_items(dir, parse::parse(content)));
    app
}

#[test]
fn up_and_down_move_the_cursor() {
    let mut app = app_with_menu("a: A\necho a\n\nb: B\necho b\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Down), &mut terminal).unwrap();

    let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
    assert_eq!(menu.current_level().selected, 1);
}

#[test]
fn enter_on_a_submenu_descends_into_it() {
    let mut app = app_with_menu("p: Parent\n{\nc: Child\necho child\n}\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Enter), &mut terminal).unwrap();

    let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
    assert_eq!(menu.current_level().items[0].title, "Child");
}

/// Regression coverage for the real request: `Right` should
/// work like `Enter` for descending into a submenu.
#[test]
fn right_on_a_submenu_descends_into_it_same_as_enter() {
    let mut app = app_with_menu("p: Parent\n{\nc: Child\necho child\n}\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Right), &mut terminal).unwrap();

    let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
    assert_eq!(menu.current_level().items[0].title, "Child");
}

/// Regression coverage for the real request: `Right` on a
/// `Commands` item must *not* run it the way `Enter` does --
/// only open it for editing (same as `F4`). An earlier version
/// made `Right` behave identically to `Enter`, which meant
/// simply navigating with the arrow keys could fire a real
/// command.
#[test]
fn right_on_a_commands_item_opens_it_for_editing_instead_of_running_it() {
    let mut app = app_with_menu("a: A\necho a\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Right), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Editing(_)), "should open the item for editing, not run it");
    let temp_path = app.user_menu_command_edit.as_ref().unwrap().temp_path.clone();
    assert_eq!(fs::read_to_string(&temp_path).unwrap(), "echo a");
}

#[test]
fn esc_at_the_top_level_closes_the_menu() {
    let mut app = app_with_menu("a: A\necho a\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Esc), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}

#[test]
fn esc_inside_a_submenu_backs_up_one_level_without_closing() {
    let mut app = app_with_menu("p: Parent\n{\nc: Child\necho child\n}\n");
    let mut terminal = dummy_terminal();
    handle_user_menu_key(&mut app, key(KeyCode::Enter), &mut terminal).unwrap();

    handle_user_menu_key(&mut app, key(KeyCode::Esc), &mut terminal).unwrap();

    let Mode::UserMenu(menu) = &app.mode else { panic!("should still be in the menu, one level up") };
    assert_eq!(menu.current_level().items[0].title, "Parent");
}

/// Regression coverage for the real request: `Left` should
/// work exactly like `Esc` for backing up one level.
#[test]
fn left_inside_a_submenu_backs_up_one_level_same_as_esc() {
    let mut app = app_with_menu("p: Parent\n{\nc: Child\necho child\n}\n");
    let mut terminal = dummy_terminal();
    handle_user_menu_key(&mut app, key(KeyCode::Enter), &mut terminal).unwrap();

    handle_user_menu_key(&mut app, key(KeyCode::Left), &mut terminal).unwrap();

    let Mode::UserMenu(menu) = &app.mode else { panic!("should still be in the menu, one level up") };
    assert_eq!(menu.current_level().items[0].title, "Parent");
}

#[test]
fn left_at_the_top_level_closes_the_menu_same_as_esc() {
    let mut app = app_with_menu("a: A\necho a\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Left), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}

/// Regression coverage for the actual point of the prompt
/// feature: an item whose commands contain `!?Label?Default!`
/// should open the prompt popup rather than running immediately.
#[test]
fn enter_on_an_item_with_a_prompt_opens_the_prompt_popup_instead_of_running() {
    let mut app = app_with_menu("c: Commit\ngit commit -m \"!?Commit title?!\"\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Enter), &mut terminal).unwrap();

    let Mode::UserMenuPrompt(prompt) = &app.mode else { panic!("expected Mode::UserMenuPrompt") };
    assert_eq!(prompt.current_label(), "Commit title");
}

#[test]
fn ignores_unrelated_keys_and_stays_open() {
    let mut app = app_with_menu("a: A\necho a\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Char('z')), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::UserMenu(_)));
}

#[test]
fn is_a_noop_outside_user_menu_mode() {
    let mut app = app_with_menu("a: A\necho a\n");
    app.mode = Mode::Browsing;
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Down), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}

#[test]
fn ins_opens_the_add_item_form() {
    let mut app = app_with_menu("a: A\necho a\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Insert), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::AddUserMenuItem(..)));
}

#[test]
fn del_removes_the_selected_item_and_persists() {
    let mut app = app_with_menu("a: A\necho a\n\nb: B\necho b\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::Delete), &mut terminal).unwrap();

    let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
    assert_eq!(menu.current_level().items.len(), 1);
    assert_eq!(menu.current_level().items[0].title, "B");
}

/// Regression coverage for the real request: `F4` on a
/// `Commands` item should open a real editor session scoped to
/// just that item's own command(s) -- not the whole
/// `LitastumMenu.toml` file, and not a bespoke single-line UI
/// form (both tried and rejected earlier).
#[test]
fn f4_on_a_commands_item_opens_the_command_in_the_real_editor() {
    let mut app = app_with_menu("a: A\necho a\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::F(4)), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Editing(_)));
    let temp_path = app.user_menu_command_edit.as_ref().unwrap().temp_path.clone();
    assert_eq!(fs::read_to_string(&temp_path).unwrap(), "echo a", "the scratch file should hold just this item's own command");
}

/// Closing the editor (`Esc`, no unsaved changes) should return
/// to the same menu, with whatever the scratch file ended up
/// holding applied to the item.
#[test]
fn closing_the_editor_after_f4_returns_to_the_menu_with_the_edited_command() {
    let mut app = app_with_menu("a: A\necho a\n\nb: B\necho b\n");
    let Mode::UserMenu(menu) = &mut app.mode else { unreachable!() };
    menu.move_down(); // cursor on "B" -- F4 should edit its command, not "A"'s
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::F(4)), &mut terminal).unwrap();
    assert!(matches!(app.mode, Mode::Editing(_)), "sanity");
    let temp_path = app.user_menu_command_edit.as_ref().unwrap().temp_path.clone();
    fs::write(&temp_path, "echo replaced").unwrap(); // simulates typing + Ctrl+S

    crate::editor::handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::UserMenu(menu) = &app.mode else {
        panic!("closing the editor should return to Mode::UserMenu, not Mode::Browsing");
    };
    assert_eq!(menu.current_level().selected, 1, "should still be on \"B\"");
    assert_eq!(menu.current_level().items[1].body, MenuItemBody::Commands(vec!["echo replaced".to_string()]));
    assert!(!temp_path.exists(), "the scratch file should have been cleaned up");
}

#[test]
fn f4_on_a_submenu_item_is_a_noop() {
    let mut app = app_with_menu("p: Parent\n{\nc: Child\necho child\n}\n");
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::F(4)), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::UserMenu(_)), "should stay on the menu, unchanged");
}

#[test]
fn f4_is_a_noop_outside_user_menu_mode() {
    let mut app = app_with_menu("a: A\necho a\n");
    app.mode = Mode::Browsing;
    let mut terminal = dummy_terminal();

    handle_user_menu_key(&mut app, key(KeyCode::F(4)), &mut terminal).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}
