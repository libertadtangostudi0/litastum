use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::{App, Mode};
use crate::command_line;
use crate::text_field;
use super::parse::{self, MenuItemBody};
use super::state::{self, AddUserMenuItemState, UserMenuPromptState, UserMenuState};


/// Key handling while browsing a (possibly nested) user menu
/// (`Mode::UserMenu`): `Up`/`Down` move, `Enter` on a submenu descends
/// into it, `Enter` on a `Commands` item substitutes `!&` (the entry
/// under the cursor) and, if any `!?Label?Default!` placeholders
/// remain, opens `Mode::UserMenuPrompt` to collect them before running
/// -- otherwise runs immediately. `Esc` backs up one level, or closes
/// the menu entirely from the top level, same shape as
/// `theming::handle_main_menu_key`. `Ins` opens the add-item form
/// (`Mode::AddUserMenuItem`); `Delete` removes the highlighted item
/// immediately (no confirmation -- this edits a config file, not real
/// user data, same reasoning `F8`'s own confirm-before-delete doesn't
/// extend to).
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

    let Mode::UserMenu(menu) = &mut app.mode else {
        return Ok(());
    };
    match key.code {
        KeyCode::Up => menu.move_up(),
        KeyCode::Down => menu.move_down(),
        KeyCode::Delete => menu.delete_selected(),
        KeyCode::Esc => {
            if !menu.back() {
                app.mode = Mode::Browsing;
            }
        }
        _ => {}
    }

    Ok(())
}


/// `Enter` on the user menu: descends into a highlighted submenu, or --
/// for a `Commands` item -- substitutes `!&`, and either runs the
/// result immediately (`command_line::run_shell_command_lines`) or, if
/// any `!?Label?Default!` placeholders remain, opens
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

    let cursor_path = app.active_panel().selected_path();
    let commands: Vec<String> = raw_commands.iter().map(|command| parse::substitute_cursor_path(command, cursor_path.as_deref())).collect();
    let prompts = parse::extract_prompts(&commands);

    debug!(item = %item_title, prompt_count = prompts.len(), "user menu: running item");
    if prompts.is_empty() {
        app.mode = Mode::Browsing;
        return command_line::run_shell_command_lines(app, terminal, &commands);
    }
    app.mode = Mode::UserMenuPrompt(UserMenuPromptState::new(commands, prompts));
    Ok(())
}


/// Key handling on a user-menu item's own `!?Label?Default!` prompt
/// popup (`Mode::UserMenuPrompt`) -- a single-line text field
/// (`text_field.rs`, same editing surface as `confirm.rs`'s transfer-
/// destination field), one prompt at a time. `Enter` accepts the
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
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Backspace => {
            let removed_selection = text_field::delete_selection(&mut prompt.value, &mut prompt.cursor, &mut prompt.selection_anchor);
            if !removed_selection {
                text_field::backspace(&mut prompt.value, &mut prompt.cursor);
            }
        }
        KeyCode::Delete => {
            let removed_selection = text_field::delete_selection(&mut prompt.value, &mut prompt.cursor, &mut prompt.selection_anchor);
            if !removed_selection {
                text_field::delete_forward(&mut prompt.value, &mut prompt.cursor);
            }
        }
        KeyCode::Left if shift => text_field::extend_selection_left(&mut prompt.cursor, &mut prompt.selection_anchor),
        KeyCode::Right if shift => text_field::extend_selection_right(&prompt.value, &mut prompt.cursor, &mut prompt.selection_anchor),
        KeyCode::Left if ctrl => {
            prompt.selection_anchor = None;
            text_field::move_word_left(&prompt.value, &mut prompt.cursor);
        }
        KeyCode::Right if ctrl => {
            prompt.selection_anchor = None;
            text_field::move_word_right(&prompt.value, &mut prompt.cursor);
        }
        KeyCode::Left => text_field::collapse_selection_left(&mut prompt.cursor, &mut prompt.selection_anchor),
        KeyCode::Right => text_field::collapse_selection_right(&prompt.value, &mut prompt.cursor, &mut prompt.selection_anchor),
        KeyCode::Home => {
            prompt.selection_anchor = None;
            text_field::move_home(&mut prompt.cursor);
        }
        KeyCode::End => {
            prompt.selection_anchor = None;
            text_field::move_end(&prompt.value, &mut prompt.cursor);
        }
        KeyCode::Char(c) if !ctrl => {
            text_field::delete_selection(&mut prompt.value, &mut prompt.cursor, &mut prompt.selection_anchor);
            text_field::insert_char(&mut prompt.value, &mut prompt.cursor, c);
        }
        _ => {}
    }

    Ok(())
}


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
/// transfer-destination field and `handle_user_menu_prompt_key` below,
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
    use std::fs;

    use crossterm::event::KeyCode;
    use ratatui::{backend::CrosstermBackend, Terminal};

    use super::*;
    use crate::explorer::UserMenuState;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn scratch_dir() -> std::path::PathBuf {
        unique_scratch_dir("user-menu-input")
    }

    /// A throwaway `Terminal` for handlers that need one just to
    /// satisfy the signature -- never actually drawn to or suspended in
    /// these tests, since every test here either stays inside the popup
    /// (no command runs) or is documented as covering the "would run"
    /// path only up to the point where it hands off to
    /// `command_line::run_shell_command_lines` (which needs a real
    /// console and isn't exercised directly here, same limitation
    /// `command_line::browsing`'s own tests already accept). The
    /// handlers' own signature hardcodes `CrosstermBackend<Stdout>`
    /// (matching `main.rs`'s real terminal type), not a generic
    /// backend, so this has to wrap real stdout too -- harmless here
    /// since nothing in these tests ever calls `.draw()` on it.
    fn dummy_terminal() -> Terminal<CrosstermBackend<std::io::Stdout>> {
        Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap()
    }

    mod handle_user_menu_key_tests {
        use super::*;

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
    }

    mod handle_user_menu_prompt_key_tests {
        use super::*;
        use crate::explorer::user_menu::parse::Prompt;
        use crate::explorer::user_menu::state::UserMenuPromptState;

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
            assert_eq!(prompt.value, "x");
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

    mod handle_confirm_port_far_menu_key_tests {
        use super::*;

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

    mod handle_add_user_menu_item_key_tests {
        use super::*;

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
}
