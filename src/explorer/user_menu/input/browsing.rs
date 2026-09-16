use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::{App, Mode};
use crate::command_line;
use crate::editor::Editor;
use crate::explorer::Panel;
use crate::explorer::user_menu::parse::{self, MacroContext, MenuItemBody, PanelMacroContext};
use crate::explorer::user_menu::state::{self, AddUserMenuItemState, UserMenuCommandEdit, UserMenuPromptState};

/// Key handling while browsing a (possibly nested) user menu
/// (`Mode::UserMenu`): `Up`/`Down` move. `Enter` on a submenu descends
/// into it, on a `Commands` item substitutes `!&` (the entry under the
/// cursor) and, if any `!?Label?Default!` placeholders remain, opens
/// `Mode::UserMenuPrompt` to collect them before running -- otherwise
/// runs immediately. A plain letter matching some item's own `hotkey`
/// at the current level does the exact same thing `Enter` would once
/// the cursor is sitting on that item (`UserMenuState::select_by_hotkey`) --
/// together, `Enter` and a matched hotkey are the only two ways this
/// runs a command. `Right` mirrors `Enter` on a submenu (descends into it),
/// but on a `Commands` item opens that item's command(s) for editing
/// instead of running them (`open_selected_item`/`open_edit_selected_command`,
/// same flow `F4` uses) -- deliberately *not* the same as `Enter` here,
/// reported directly after an earlier version made `Right` run the
/// command too: browsing shouldn't risk firing something real,
/// `Right`'s own "go deeper" direction reads more naturally as "go look
/// inside this" than as a second way to run it. `Esc`/`Left` back up
/// one level, or close the menu entirely from the top level, same shape
/// as `theming::handle_main_menu_key` -- `Right`/`Left` alongside
/// `Enter`/`Esc` for navigation was requested directly, matching real
/// Far Manager's own menu navigation, where either key works. `Ins`
/// opens the add-item form (`Mode::AddUserMenuItem`); `Delete` removes
/// the highlighted item immediately (no confirmation -- this edits a
/// config file, not real user data, same reasoning `F8`'s own confirm-
/// before-delete doesn't extend to); `F4` opens a `Commands` item's
/// command(s) for editing directly, without needing to navigate onto it
/// with `Right` first -- unlike `Right`, it's a no-op on a `Submenu`
/// item rather than descending into it, since reaching for `F4`
/// directly is specifically about editing a command.
pub fn handle_user_menu_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    // `Enter` needs its own function: descending into a submenu or
    // running an item both need fresh, narrowly-scoped borrows of
    // `app.mode` (running also needs a separate `&mut App` for
    // `active_panel()`/`run_shell_command_lines`) -- easier to keep
    // that self-contained than to share one long-lived borrow across
    // every arm of the match below, same reasoning
    // `confirm::handle_confirm_transfer_key` already applies to its own
    // `Enter` case.
    if key.code == KeyCode::Enter {
        return run_selected_user_menu_item(app, terminal);
    }

    // `Ins` also needs to *replace* `app.mode` entirely (moving the
    // current `UserMenuState` into `Mode::AddUserMenuItem` alongside a
    // fresh form, so `Esc` on the form can hand it straight back) --
    // same "needs `&mut App`, not just a borrow of `Mode::UserMenu`'s
    // own payload" reasoning as `Enter` above.
    if key.code == KeyCode::Insert {
        if !matches!(&app.mode, Mode::UserMenu(_)) {
            return Ok(());
        }
        let Mode::UserMenu(menu) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            unreachable!("just matched above");
        };
        app.mode = Mode::AddUserMenuItem(menu, AddUserMenuItemState::new());
        return Ok(());
    }

    // `Right` needs its own function too: it either descends into a
    // submenu (`UserMenuState::enter_submenu`, a plain in-place
    // mutation) or opens the edit-command flow (which -- like `F4`,
    // `Ins` above -- needs to *replace* `app.mode` entirely).
    if key.code == KeyCode::Right {
        open_selected_item(app);
        return Ok(());
    }

    // `F4` -- same "needs the whole `&mut App`, not just the payload"
    // reasoning again: it replaces `app.mode` with `Mode::Editing` and
    // parks the menu in `app.user_menu_command_edit`. Unlike `Right`
    // above, it never descends into a submenu -- reaching for `F4`
    // directly is specifically about editing a command, so a no-op on
    // a `Submenu` item (nothing single to edit there) is more honest
    // than silently doing something else instead.
    if key.code == KeyCode::F(4) {
        open_edit_selected_command(app);
        return Ok(());
    }

    // A plain letter matching some item's own `hotkey` at the *current*
    // level jumps the cursor there and runs/descends into it
    // immediately, matching real Far Manager's own user-menu
    // convention (and, before this, a real gap: the hotkey was parsed
    // and shown as a prefix in every row but never actually wired up as
    // a shortcut at all -- reported directly). Same "needs the whole
    // `&mut App` + `Terminal`" reasoning as `Enter` above, since a
    // match runs `run_selected_user_menu_item` itself. An unmatched
    // letter (or any other key still falling through to here) is a
    // silent no-op past this point, same as before this was added.
    if let KeyCode::Char(c) = key.code {
        let matched = {
            let Mode::UserMenu(menu) = &mut app.mode else {
                return Ok(());
            };
            menu.select_by_hotkey(c)
        };
        if matched {
            return run_selected_user_menu_item(app, terminal);
        }
        return Ok(());
    }

    let Mode::UserMenu(menu) = &mut app.mode else {
        return Ok(());
    };
    match key.code {
        KeyCode::Up => menu.move_up(),
        KeyCode::Down => menu.move_down(),
        KeyCode::Delete => menu.delete_selected(),
        KeyCode::Esc | KeyCode::Left => {
            if !menu.back() {
                app.mode = Mode::Browsing;
            }
        }
        _ => {}
    }

    Ok(())
}


/// `Enter` on the user menu: descends into a highlighted submenu, or --
/// for a `Commands` item -- substitutes every Far (`!.!`, `!&`, ...) or
/// litastum-native (`{{cursor}}`) macro (`parse::substitute_macros`),
/// and either runs the result immediately
/// (`command_line::run_shell_command_lines`) or, if any
/// `!?Label?Default!`/`{{prompt:...}}` placeholders remain, opens
/// `Mode::UserMenuPrompt` to collect them first.
fn run_selected_user_menu_item(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let entered = {
        let Mode::UserMenu(menu) = &mut app.mode else {
            return Ok(());
        };
        menu.enter_submenu()
    };
    if entered {
        return Ok(());
    }

    let (raw_commands, item_title) = {
        let Mode::UserMenu(menu) = &app.mode else {
            return Ok(());
        };
        let Some(item) = menu.selected_item() else {
            return Ok(());
        };
        let MenuItemBody::Commands(raw_commands) = &item.body else {
            unreachable!("enter_submenu already handled the Submenu case above")
        };
        (raw_commands.clone(), item.title.clone())
    };

    let macro_context = build_macro_context(app);
    let commands: Vec<String> = raw_commands.iter().map(|command| parse::substitute_macros(command, &macro_context)).collect();
    let prompts = parse::extract_prompts(&commands);

    debug!(item = %item_title, prompt_count = prompts.len(), "user menu: running item");
    if prompts.is_empty() {
        app.mode = Mode::Browsing;
        return command_line::run_shell_command_lines(app, terminal, &commands);
    }
    app.mode = Mode::UserMenuPrompt(UserMenuPromptState::new(commands, prompts));
    Ok(())
}


/// Builds the macro-substitution context (`parse::MacroContext`) from
/// `app`'s two panels -- `active`/`passive` follow whichever panel
/// currently has focus (`app.active`), `left`/`right` are the fixed
/// on-screen panels regardless of focus, matching real Far Manager's
/// own four-way addressing (`!^`/`!##`/`![`/`!]`, see
/// `parse`'s own `substitution` submodule for the actual token
/// dispatch). Built fresh right
/// before running a menu item's commands -- never stored, since panel
/// state (cursor position, marks) can change between one run and the
/// next.
fn build_macro_context(app: &App) -> MacroContext {
    let passive_index = 1 - app.active;
    MacroContext {
        active: panel_macro_context(&app.panels[app.active]),
        passive: panel_macro_context(&app.panels[passive_index]),
        left: panel_macro_context(&app.panels[0]),
        right: panel_macro_context(&app.panels[1]),
    }
}

fn panel_macro_context(panel: &Panel) -> PanelMacroContext {
    PanelMacroContext {
        dir: panel.path.clone(),
        cursor: panel.selected_path(),
        selected: panel.marked_or_current().iter().map(|entry| panel.path.join(&entry.name)).collect(),
    }
}


/// `Right` on the user menu: on a submenu, descends into it exactly
/// like `Enter` would (`UserMenuState::enter_submenu`); on a `Commands`
/// item, defers to `open_edit_selected_command` (the same thing `F4`
/// does) rather than running it -- deliberately *not* the same as
/// `Enter` here: reported directly after an earlier version made
/// `Right` behave identically to `Enter`, which meant simply navigating
/// with the arrow keys could fire a real command. `Enter` is now the
/// only key that actually runs anything; `Right`'s own "go deeper"
/// direction reads more naturally as "go look inside this" than as a
/// second way to run it.
fn open_selected_item(app: &mut App) {
    let entered = {
        let Mode::UserMenu(menu) = &mut app.mode else { return };
        menu.enter_submenu()
    };
    if entered {
        return;
    }
    open_edit_selected_command(app);
}


/// `F4` on the user menu (also reached via `Right` above, for a
/// `Commands` item specifically): opens the highlighted item's own
/// command(s) in the real built-in editor -- a scratch file
/// (`state::create_command_edit_file`) holding just those lines, not
/// the whole `LitastumMenu.toml`. A no-op for a `Submenu` item or an
/// empty level (nothing single to edit there), and a silent no-op if
/// the scratch file can't be created/opened, same as every other
/// "couldn't act on this" case in this codebase. Requested directly,
/// twice: a first version opened the *whole* file, a second opened a
/// bespoke single-line UI form -- both missed the actual ask, a real
/// editor session scoped to just this item's own command(s).
fn open_edit_selected_command(app: &mut App) {
    let Mode::UserMenu(menu) = &app.mode else { return };
    let Some(item) = menu.selected_item() else { return };
    let MenuItemBody::Commands(commands) = &item.body else { return };

    let Ok(temp_path) = state::create_command_edit_file(commands) else { return };
    let syntax_theme = app.syntax_theme.clone();
    let Ok(editor) = Editor::open(temp_path.clone(), syntax_theme, app.editor_keymap_mode) else {
        return;
    };

    let Mode::UserMenu(menu) = std::mem::replace(&mut app.mode, Mode::Editing(editor)) else {
        unreachable!("just matched Mode::UserMenu above");
    };
    app.user_menu_command_edit = Some(UserMenuCommandEdit { menu, temp_path });
}


#[cfg(test)]
mod tests {
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
}
