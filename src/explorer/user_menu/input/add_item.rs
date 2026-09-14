use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Mode};
use crate::text_field;

/// Key handling on the `Ins` add-item form (`Mode::AddUserMenuItem`):
/// `Esc` cancels back to browsing the menu untouched at any stage;
/// `Enter` on the title field advances to the command field (a no-op
/// on a blank title); `Enter` on the command field inserts the
/// finished item (`UserMenuState::insert_item`, which also persists)
/// and returns to browsing with it selected. Anything else edits
/// whichever field is currently active.
pub fn handle_add_user_menu_item_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        let Mode::AddUserMenuItem(menu, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            return Ok(());
        };
        app.mode = Mode::UserMenu(menu);
        return Ok(());
    }

    if key.code == KeyCode::Enter {
        let Mode::AddUserMenuItem(_, form) = &mut app.mode else {
            return Ok(());
        };
        if form.is_title_stage() {
            form.advance_from_title();
            return Ok(());
        }

        let Mode::AddUserMenuItem(mut menu, form) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            unreachable!("just matched above");
        };
        menu.insert_item(form.finish());
        app.mode = Mode::UserMenu(menu);
        return Ok(());
    }

    let Mode::AddUserMenuItem(_, form) = &mut app.mode else {
        return Ok(());
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if form.is_title_stage() {
        edit_field(&mut form.title, &mut form.title_cursor, &mut form.title_selection_anchor, key, ctrl);
    } else {
        edit_field(&mut form.command, &mut form.command_cursor, &mut form.command_selection_anchor, key, ctrl);
    }

    Ok(())
}


/// Shared single-line text-field editing (insert/backspace/delete/
/// selection/word-movement) -- same behavior as `confirm.rs`'s own
/// transfer-destination field and `input::prompt::handle_user_menu_prompt_key`,
/// factored out here since the add-item form has two independent
/// fields (title, command) that both need exactly this.
fn edit_field(value: &mut String, cursor: &mut usize, selection_anchor: &mut Option<usize>, key: KeyEvent, ctrl: bool) {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Backspace => {
            if !text_field::delete_selection(value, cursor, selection_anchor) {
                text_field::backspace(value, cursor);
            }
        }
        KeyCode::Delete => {
            if !text_field::delete_selection(value, cursor, selection_anchor) {
                text_field::delete_forward(value, cursor);
            }
        }
        KeyCode::Left if shift => text_field::extend_selection_left(cursor, selection_anchor),
        KeyCode::Right if shift => text_field::extend_selection_right(value, cursor, selection_anchor),
        KeyCode::Left if ctrl => {
            *selection_anchor = None;
            text_field::move_word_left(value, cursor);
        }
        KeyCode::Right if ctrl => {
            *selection_anchor = None;
            text_field::move_word_right(value, cursor);
        }
        KeyCode::Left => text_field::collapse_selection_left(cursor, selection_anchor),
        KeyCode::Right => text_field::collapse_selection_right(value, cursor, selection_anchor),
        KeyCode::Home => {
            *selection_anchor = None;
            text_field::move_home(cursor);
        }
        KeyCode::End => {
            *selection_anchor = None;
            text_field::move_end(value, cursor);
        }
        KeyCode::Char(c) if !ctrl => {
            text_field::delete_selection(value, cursor, selection_anchor);
            text_field::insert_char(value, cursor, c);
        }
        _ => {}
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::user_menu::parse::{self, MenuItemBody};
    use crate::explorer::user_menu::input::scratch_dir;
    use crate::explorer::user_menu::state::{AddUserMenuItemState, UserMenuState};
    use crate::test_support::{key, test_app};

    fn app_in_add_form(existing_content: &str) -> App {
        let dir = scratch_dir();
        let menu = UserMenuState::from_items(dir.clone(), parse::parse(existing_content));
        let mut app = test_app(dir);
        app.mode = Mode::AddUserMenuItem(menu, AddUserMenuItemState::new());
        app
    }

    #[test]
    fn typing_edits_the_title_field() {
        let mut app = app_in_add_form("a: A\necho a\n");

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::AddUserMenuItem(_, form) = &app.mode else { panic!("expected Mode::AddUserMenuItem") };
        assert_eq!(form.title, "x");
    }

    #[test]
    fn enter_on_a_blank_title_does_not_advance() {
        let mut app = app_in_add_form("a: A\necho a\n");

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::AddUserMenuItem(_, form) = &app.mode else { panic!("expected Mode::AddUserMenuItem") };
        assert!(form.is_title_stage());
    }

    #[test]
    fn enter_advances_to_the_command_stage_then_typing_edits_it() {
        let mut app = app_in_add_form("a: A\necho a\n");
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Enter)).unwrap();

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('c'))).unwrap();

        let Mode::AddUserMenuItem(_, form) = &app.mode else { panic!("expected Mode::AddUserMenuItem") };
        assert!(!form.is_title_stage());
        assert_eq!(form.command, "c");
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

        let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
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

        let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
        let new_item = menu.current_level().items.iter().find(|i| i.title == "git").unwrap();
        assert_eq!(new_item.body, MenuItemBody::Submenu(Vec::new()));
    }

    #[test]
    fn esc_cancels_back_to_the_menu_unchanged() {
        let mut app = app_in_add_form("a: A\necho a\n");
        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Mode::UserMenu(menu) = &app.mode else { panic!("expected Mode::UserMenu") };
        assert_eq!(menu.current_level().items.len(), 1, "nothing should have been added");
    }

    #[test]
    fn is_a_noop_outside_add_item_mode() {
        let mut app = app_in_add_form("a: A\necho a\n");
        app.mode = Mode::Browsing;

        handle_add_user_menu_item_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    /// `app_in_add_form` above always builds a fresh scratch dir;
    /// this variant takes an existing one, needed by the one test
    /// that checks the persisted file afterward.
    fn app_in_add_form_at(dir: &std::path::Path, existing_content: &str) -> App {
        let menu = UserMenuState::from_items(dir.to_path_buf(), parse::parse(existing_content));
        let mut app = test_app(dir.to_path_buf());
        app.mode = Mode::AddUserMenuItem(menu, AddUserMenuItemState::new());
        app
    }
}
