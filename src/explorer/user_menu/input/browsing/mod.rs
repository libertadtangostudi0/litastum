use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::editor::Editor;
use crate::explorer::Panel;
use crate::explorer::user_menu::parse::{self, MacroContext, MenuItemBody, PanelMacroContext};
use crate::explorer::user_menu::state::{self, AddUserMenuItemState, UserMenuCommandEdit, UserMenuPromptState};

/// Keys while browsing the user menu (`Overlay::UserMenu`):
/// - `Up`/`Down` move; `Enter` or an item's hotkey descends into a submenu
///   or runs a `Commands` item (collecting `!?Label?Default!` answers
///   first via `Overlay::UserMenuPrompt`);
/// - `Right` descends into a submenu but *edits* a `Commands` item (like
///   `F4`) -- arrow navigation must never run a command;
/// - `Esc`/`Left` go up a level or close; `Ins` adds an item; `Delete`
///   removes one without confirmation (it's a config file, not user data);
///   `F4` edits a `Commands` item's commands, no-op on a submenu.
///
/// History: docs/history/user-menu.md.
pub fn handle_user_menu_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    // Its own function: descending and running each need short, separate
    // borrows of `app` (as in `confirm::handle_confirm_transfer_key`).
    if key.code == KeyCode::Enter {
        return run_selected_user_menu_item(app);
    }

    // `Ins` also needs to *replace* `app.mode` entirely (moving the
    // current `UserMenuState` into `Overlay::AddUserMenuItem` alongside a
    // fresh form, so `Esc` on the form can hand it straight back) --
    // same "needs `&mut App`, not just a borrow of `Overlay::UserMenu`'s
    // own payload" reasoning as `Enter` above.
    if key.code == KeyCode::Insert {
        if !matches!(&app.overlay, Some(Overlay::UserMenu(_))) {
            return Ok(Effect::None);
        }
        let Some(Overlay::UserMenu(menu)) = app.overlay.take() else {
            unreachable!("just matched above");
        };
        app.overlay = Some(Overlay::AddUserMenuItem(menu, AddUserMenuItemState::new()));
        return Ok(Effect::None);
    }

    // `Right` needs its own function too: it either descends into a
    // submenu (`UserMenuState::enter_submenu`, a plain in-place
    // mutation) or opens the edit-command flow (which -- like `F4`,
    // `Ins` above -- needs to *replace* `app.mode` entirely).
    if key.code == KeyCode::Right {
        open_selected_item(app);
        return Ok(Effect::None);
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
        return Ok(Effect::None);
    }

    // A letter matching an item's hotkey at the current level selects it
    // and acts like `Enter` (Far's convention). Anything else is ignored.
    if let KeyCode::Char(c) = key.code {
        let matched = {
            let Some(Overlay::UserMenu(menu)) = &mut app.overlay else {
                return Ok(Effect::None);
            };
            menu.select_by_hotkey(c)
        };
        if matched {
            return run_selected_user_menu_item(app);
        }
        return Ok(Effect::None);
    }

    let Some(Overlay::UserMenu(menu)) = &mut app.overlay else {
        return Ok(Effect::None);
    };
    match key.code {
        KeyCode::Up => menu.move_up(),
        KeyCode::Down => menu.move_down(),
        KeyCode::Delete => menu.delete_selected(),
        KeyCode::Esc | KeyCode::Left => {
            if !menu.back() {
                app.overlay = None;
            }
        }
        _ => {}
    }

    Ok(Effect::None)
}


/// `Enter` on the user menu: descends into a highlighted submenu, or --
/// for a `Commands` item -- substitutes every Far (`!.!`, `!&`, ...) or
/// litastum-native (`{{cursor}}`) macro (`parse::substitute_macros`),
/// and either runs the result immediately (`Effect::RunShell`) or, if any
/// `!?Label?Default!`/`{{prompt:...}}` placeholders remain, opens
/// `Overlay::UserMenuPrompt` to collect them first.
fn run_selected_user_menu_item(app: &mut App) -> Result<Effect> {
    let entered = {
        let Some(Overlay::UserMenu(menu)) = &mut app.overlay else {
            return Ok(Effect::None);
        };
        menu.enter_submenu()
    };
    if entered {
        return Ok(Effect::None);
    }

    let (raw_commands, item_title) = {
        let Some(Overlay::UserMenu(menu)) = &app.overlay else {
            return Ok(Effect::None);
        };
        let Some(item) = menu.selected_item() else {
            return Ok(Effect::None);
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
        app.overlay = None;
        return Ok(Effect::RunShell(commands));
    }
    app.overlay = Some(Overlay::UserMenuPrompt(UserMenuPromptState::new(commands, prompts)));
    Ok(Effect::None)
}


/// The macro context from both panels: `active`/`passive` follow focus,
/// `left`/`right` are fixed on screen (Far's `!^`/`!##`/`![`/`!]`). Built
/// fresh before each run, since cursor and marks change.
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


/// `Right`: descends into a submenu; on a `Commands` item, opens it for
/// editing instead of running it. History: docs/history/user-menu.md.
fn open_selected_item(app: &mut App) {
    let entered = {
        let Some(Overlay::UserMenu(menu)) = &mut app.overlay else { return };
        menu.enter_submenu()
    };
    if entered {
        return;
    }
    open_edit_selected_command(app);
}


/// `F4` (or `Right` on a `Commands` item): opens just this item's commands
/// in the built-in editor, via a scratch file (`state::create_command_edit_file`).
/// No-op on a submenu, an empty level, or a failed scratch file.
/// History: docs/history/user-menu.md.
fn open_edit_selected_command(app: &mut App) {
    let Some(Overlay::UserMenu(menu)) = &app.overlay else { return };
    let Some(item) = menu.selected_item() else { return };
    let MenuItemBody::Commands(commands) = &item.body else { return };

    let Ok(temp_path) = state::create_command_edit_file(commands) else { return };
    let syntax_theme = app.syntax_theme.clone();
    let Ok(editor) = Editor::open(temp_path.clone(), syntax_theme, app.settings.editor_keymap_mode) else {
        return;
    };

    let Some(Overlay::UserMenu(menu)) = app.overlay.take() else {
        unreachable!("just matched Overlay::UserMenu above");
    };
    app.mode = Mode::Editing(editor);
    app.user_menu_command_edit = Some(UserMenuCommandEdit { menu, temp_path });
}


#[cfg(test)]
mod tests;
