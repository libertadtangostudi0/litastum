mod app;
mod command;
mod command_line;
mod config;
mod editor;
mod editor_keymap;
mod fs_ops;
mod keymap;
mod logging;
mod menu;
mod panel;
mod scheme;
mod shell;
mod text_field;
mod theme;
mod theme_menu;
mod ui;

use std::fs;
use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use app::{App, Mode, ShellMenu, TransferOp};
use editor_keymap::EditorCommand;
use theme_menu::ThemeMenu;


fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let start_dir = std::env::current_dir()?;

    let mut terminal = setup_terminal()?;
    let (theme, syntax_theme) = config::load_active_theme();
    let mut app = App::new(start_dir, theme, syntax_theme)?;
    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}


fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // A thin bar matches a normal text caret; edtui's own cursor
    // highlight is turned off in editor.rs so this is what's visible
    // while editing (blinking, so it's still findable at a glance).
    execute!(stdout, EnterAlternateScreen, SetCursorStyle::BlinkingBar)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}


fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape
    )?;
    Ok(())
}


fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        let mut columns = [1usize; 2];
        terminal.draw(|frame| columns = ui::draw(frame, app))?;
        for (panel, cols) in app.panels.iter_mut().zip(columns) {
            panel.set_columns(cols);
        }
        handle_event(app, terminal)?;
    }
    Ok(())
}


fn handle_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match &app.mode {
        Mode::Editing(_) => handle_editor_key(app, key),
        Mode::ConfirmDiscard(_) => handle_confirm_discard_key(app, key),
        Mode::ConfirmDelete(_) => handle_confirm_delete_key(app, key),
        Mode::ConfirmTransfer(_) => handle_confirm_transfer_key(app, key),
        Mode::MainMenu(_) => handle_main_menu_key(app, key),
        Mode::ThemeMenu(_) => handle_theme_menu_key(app, key),
        Mode::ShellMenu(_) => handle_shell_menu_key(app, key),
        Mode::Browsing => handle_browsing_key(app, key, terminal),
    }
}


/// Key handling in the browser: `Ctrl+P` opens the shell picker (see
/// `handle_shell_menu_key`); `Enter` with something typed runs it
/// (`run_command_line`), otherwise the fixed `keymap::resolve` table
/// (arrows, Tab, F4/F9/F10, and `Enter` on an *empty* command line —
/// `EnterSelected`, unchanged) takes over; anything that table doesn't
/// bind — plain characters, `Backspace`, `Esc` — edits the always-live
/// command line at the bottom of the browser (Far Manager-style; see
/// `.claude/rules/litastum-stack.md`). This is also why `q` no longer
/// quits on its own (`keymap.rs`) — a bare letter now types into the
/// command line like any other.
fn handle_browsing_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    debug!(?key, "browsing key");

    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.mode = Mode::ShellMenu(ShellMenu { selected: app.active_shell });
        return Ok(());
    }

    // Shift+F6 (rename) vs plain F6 (move) only differ by modifier --
    // keymap::resolve's table keys off KeyCode alone, so this one has
    // to be special-cased ahead of it, same as Ctrl+P above.
    if key.code == KeyCode::F(6) && key.modifiers.contains(KeyModifiers::SHIFT) {
        return command::execute(keymap::Command::RenameSelected, app);
    }

    if key.code == KeyCode::Enter && !app.command_line.is_empty() {
        return run_command_line(app, terminal);
    }

    if let Some(cmd) = keymap::resolve(key.code) {
        debug!(?cmd, "browsing command");
        return command::execute(cmd, app);
    }

    match key.code {
        KeyCode::Esc => app.command_line.clear(),
        KeyCode::Backspace => command_line::backspace(&mut app.command_line),
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            command_line::insert_char(&mut app.command_line, c);
        }
        _ => {}
    }

    Ok(())
}


/// Runs whatever's typed in `app.command_line`: `cd`-shaped input
/// changes the active panel's directory directly (`Panel::change_dir`
/// — a spawned shell's own `cd` could never affect our process, so
/// this has to be handled ourselves, same as Far Manager does it);
/// anything else suspends the TUI and hands the console to the
/// configured shell profile (`app.shell_profiles[app.active_shell]`),
/// inheriting stdio so interactive programs (an editor, a REPL, ...)
/// work too, not just one-shot commands.
fn run_command_line(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let input = app.command_line.trim().to_string();
    app.command_line.clear();
    if input.is_empty() {
        return Ok(());
    }

    if let Some(target) = command_line::parse_cd_target(&input) {
        debug!(target, "command line: cd");
        app.active_panel().change_dir(target)?;
        return Ok(());
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    debug!(shell = profile.name, %input, cwd = %cwd.display(), "command line: running");

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    println!("{}> {input}", cwd.display());
    let status = std::process::Command::new(&profile.program)
        .args(&profile.args_prefix)
        .arg(&input)
        .current_dir(&cwd)
        .status();
    match status {
        Ok(status) if !status.success() => {
            debug!(?status, "command exited non-zero");
        }
        Err(err) => println!("failed to launch '{}': {err}", profile.program),
        Ok(_) => {}
    }
    println!("\nPress any key to continue...");

    // Wait for one real keypress before redrawing -- otherwise output
    // that scrolled by fast is gone the instant the panels repaint.
    loop {
        if let Event::Key(k) = event::read()? {
            if k.kind == KeyEventKind::Press {
                break;
            }
        }
    }

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    app.active_panel().reload()?;
    Ok(())
}


/// Key handling while a file is open in the built-in editor. `Ctrl+S`
/// and `Esc` are the only things `main.rs` still resolves itself (see
/// `editor_keymap::resolve`) — everything else, including copy/cut/
/// paste/selection, is `edtui`'s own concern once forwarded to
/// `Editor::input`. `Esc` is special-cased further: with an active
/// selection it's forwarded too (so `edtui`'s own binding cancels the
/// selection), only closing the editor once there's nothing selected.
fn handle_editor_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let command = editor_keymap::resolve(key);
    debug!(?key, ?command, "editor key");

    if command == EditorCommand::Close {
        let has_selection = matches!(&app.mode, Mode::Editing(editor) if editor.has_selection());
        if has_selection {
            let Mode::Editing(active_editor) = &mut app.mode else {
                return Ok(());
            };
            active_editor.input(key);
            return Ok(());
        }
        return close_editor_or_confirm(app);
    }

    let Mode::Editing(active_editor) = &mut app.mode else {
        return Ok(());
    };

    match command {
        EditorCommand::Close => unreachable!("handled above"),
        EditorCommand::Save => active_editor.save()?,
        EditorCommand::Forward => active_editor.input(key),
    }

    Ok(())
}


/// `Esc` in the editor: closes straight back to browsing if the buffer
/// has no unsaved changes, otherwise moves to `Mode::ConfirmDiscard`
/// instead of discarding them silently.
fn close_editor_or_confirm(app: &mut App) -> Result<()> {
    let Mode::Editing(editor) = &app.mode else {
        return Ok(());
    };

    if !editor.is_dirty() {
        app.mode = Mode::Browsing;
        app.active_panel().reload()?;
        return Ok(());
    }

    debug!("editor close: unsaved changes, asking to confirm discard");
    let Mode::Editing(editor) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::Editing above");
    };
    app.mode = Mode::ConfirmDiscard(editor);
    Ok(())
}


/// Key handling on the "discard unsaved changes?" prompt: `Y` discards
/// and returns to browsing, `N`/`Esc` cancels back into the editor with
/// nothing lost, anything else is ignored.
fn handle_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use editor_keymap::ConfirmDiscardCommand;

    let command = editor_keymap::resolve_confirm_discard(key);
    debug!(?key, ?command, "confirm-discard key");

    match command {
        ConfirmDiscardCommand::Discard => {
            app.mode = Mode::Browsing;
            app.active_panel().reload()?;
        }
        ConfirmDiscardCommand::Cancel => {
            let Mode::ConfirmDiscard(editor) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::ConfirmDiscard");
            };
            app.mode = Mode::Editing(editor);
        }
        ConfirmDiscardCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling on the F8 "delete this?" prompt: `Y` actually deletes
/// (a file via `fs::remove_file`, a directory recursively via
/// `fs::remove_dir_all` — no separate "is it empty" case, matching Far
/// Manager's own F8 which recurses without asking twice) and reloads
/// the panel; `N`/`Esc` cancels with nothing touched. A failed delete
/// (permissions, a file in use, ...) is logged rather than crashing —
/// there's no status-bar message surface yet to show it to the user
/// (see `TODO.md`'s non-UTF-8-file gap, same underlying limitation).
fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use keymap::ConfirmDeleteCommand;

    let Mode::ConfirmDelete(pending) = &app.mode else {
        return Ok(());
    };

    let command = keymap::resolve_confirm_delete(key);
    debug!(?key, ?command, path = %pending.path.display(), "confirm-delete key");

    match command {
        ConfirmDeleteCommand::Confirm => {
            let Mode::ConfirmDelete(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::ConfirmDelete above");
            };
            let result = if pending.is_dir {
                fs::remove_dir_all(&pending.path)
            } else {
                fs::remove_file(&pending.path)
            };
            if let Err(err) = result {
                debug!(path = %pending.path.display(), %err, "delete failed");
            }
            app.active_panel().reload()?;
        }
        ConfirmDeleteCommand::Cancel => app.mode = Mode::Browsing,
        ConfirmDeleteCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling on the F5/F6 "copy/move to?" prompt: the destination
/// line gets a real cursor (`text_field.rs`, not the command line's
/// own append/backspace-only editing — see that module's doc for why
/// this popup gets one and the always-live command line doesn't) —
/// `Left`/`Right` move a character, `Ctrl+Left`/`Ctrl+Right` a word,
/// `Home`/`End` to the edges, `Backspace`/`Delete` remove around the
/// cursor. `Enter` performs the transfer (`fs_ops::copy_entry`/
/// `move_entry`) and reloads *both* panels (the destination side
/// always needs it, and a move also changes the source side); `Esc`
/// cancels with nothing touched. A failed transfer is only logged, same
/// as `handle_confirm_delete_key` — no status-bar surface exists yet.
fn handle_confirm_transfer_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Enter => {
            let Mode::ConfirmTransfer(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                return Ok(());
            };
            let destination = std::path::PathBuf::from(pending.destination.trim());
            debug!(
                source = %pending.source.display(),
                destination = %destination.display(),
                op = ?pending.operation,
                "confirm-transfer: running"
            );
            let result = match pending.operation {
                TransferOp::Copy => fs_ops::copy_entry(&pending.source, &destination, pending.is_dir),
                TransferOp::Move => fs_ops::move_entry(&pending.source, &destination, pending.is_dir),
            };
            if let Err(err) = result {
                debug!(source = %pending.source.display(), destination = %destination.display(), %err, "transfer failed");
            }
            for panel in &mut app.panels {
                panel.reload()?;
            }
        }
        KeyCode::Esc => app.mode = Mode::Browsing,
        // Backspace/Delete remove the active selection instead of one
        // character, if there is one -- text_field::delete_selection
        // reports whether it did anything, so the single-character path
        // only runs when there wasn't a selection to consume instead.
        KeyCode::Backspace => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                let removed_selection =
                    text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
                if !removed_selection {
                    text_field::backspace(&mut pending.destination, &mut pending.cursor);
                }
            }
        }
        KeyCode::Delete => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                let removed_selection =
                    text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
                if !removed_selection {
                    text_field::delete_forward(&mut pending.destination, &mut pending.cursor);
                }
            }
        }
        // Shift+Left/Right (selection) is checked ahead of Ctrl+Left/
        // Right and plain Left/Right below, same reason Ctrl+P is
        // checked ahead of the browsing keymap table -- KeyCode::Left
        // alone can't distinguish "extend selection" from "move" or
        // "jump a word".
        KeyCode::Left if shift => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                text_field::extend_selection_left(&mut pending.cursor, &mut pending.selection_anchor);
            }
        }
        KeyCode::Right if shift => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                text_field::extend_selection_right(&pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            }
        }
        KeyCode::Left if ctrl => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                pending.selection_anchor = None;
                text_field::move_word_left(&pending.destination, &mut pending.cursor);
            }
        }
        KeyCode::Right if ctrl => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                pending.selection_anchor = None;
                text_field::move_word_right(&pending.destination, &mut pending.cursor);
            }
        }
        // Plain Left/Right with a selection active collapses to that
        // selection's near edge (standard editor behavior) rather than
        // moving one further character past it.
        KeyCode::Left => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                text_field::collapse_selection_left(&mut pending.cursor, &mut pending.selection_anchor);
            }
        }
        KeyCode::Right => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                text_field::collapse_selection_right(&pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            }
        }
        KeyCode::Home => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                pending.selection_anchor = None;
                text_field::move_home(&mut pending.cursor);
            }
        }
        KeyCode::End => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                pending.selection_anchor = None;
                text_field::move_end(&pending.destination, &mut pending.cursor);
            }
        }
        // Typing over an active selection replaces it, like any normal
        // text field -- delete it first, then insert at the (now
        // collapsed) cursor.
        KeyCode::Char(c) if !ctrl => {
            if let Mode::ConfirmTransfer(pending) = &mut app.mode {
                text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
                text_field::insert_char(&mut pending.destination, &mut pending.cursor, c);
            }
        }
        _ => {}
    }

    Ok(())
}


/// Key handling on the F9 top menu (`menu.rs`): `Up`/`Down` move,
/// `Enter` descends into a submenu or, at the deepest level ("Color
/// schemes"), opens `Mode::ThemeMenu`; `Esc` backs up one level, or
/// closes the menu entirely if already at the top.
fn handle_main_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use menu::{MenuCommand, MenuLevel};

    let Mode::MainMenu(menu_state) = &mut app.mode else {
        return Ok(());
    };

    let command = menu::resolve(key);
    debug!(?key, ?command, "main menu key");

    match command {
        MenuCommand::Up => menu_state.move_up(),
        MenuCommand::Down => menu_state.move_down(),
        MenuCommand::Back => {
            if !menu_state.back() {
                app.mode = Mode::Browsing;
            }
        }
        MenuCommand::Select => {
            let level = menu_state.level;
            match level {
                MenuLevel::Main => menu_state.enter_settings(),
                // Only one item at Settings level today ("Color
                // schemes"), so Select unconditionally opens it --
                // revisit once Settings grows more than one item.
                MenuLevel::Settings => app.mode = Mode::ThemeMenu(ThemeMenu::open()),
            }
        }
        MenuCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling on the F9 color-scheme picker: `Enter` applies the
/// highlighted theme as both interface and editor theme, `I`/`E` apply
/// just one side (see `theme_menu::ThemeMenuCommand`), `Esc` closes
/// without changing anything. Applying updates `app.theme`/
/// `app.syntax_theme` immediately — no restart — and persists the
/// choice to `config.json` on a best-effort basis (`config.rs` logs and
/// carries on if that write fails; the live preview still applies).
fn handle_theme_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use theme_menu::ThemeMenuCommand;

    let Mode::ThemeMenu(menu) = &mut app.mode else {
        return Ok(());
    };

    let command = theme_menu::resolve(key);
    debug!(?key, ?command, "theme menu key");

    match command {
        ThemeMenuCommand::Up => menu.move_up(),
        ThemeMenuCommand::Down => menu.move_down(),
        ThemeMenuCommand::Close => app.mode = Mode::Browsing,
        ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyInterfaceOnly | ThemeMenuCommand::ApplyEditorOnly => {
            let Some(name) = menu.selected_theme().map(str::to_string) else {
                return Ok(());
            };
            if matches!(command, ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyInterfaceOnly) {
                if let Some(theme) = config::set_interface_theme(&name) {
                    app.theme = theme;
                }
            }
            if matches!(command, ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyEditorOnly) {
                if let Some(syntax_theme) = config::set_editor_theme(&name) {
                    app.syntax_theme = Some(syntax_theme);
                }
            }
            app.mode = Mode::Browsing;
        }
        ThemeMenuCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling on the `Ctrl+P` shell picker: `Up`/`Down` to move,
/// `Enter` sets `app.active_shell` and closes, `Esc` cancels. Not
/// persisted to `config.json` — resets to the platform default each
/// run (see the plan doc / `.claude/rules/litastum-stack.md`).
fn handle_shell_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ShellMenu(menu) = &mut app.mode else {
        return Ok(());
    };
    debug!(?key, selected = menu.selected, "shell menu key");

    match key.code {
        KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
        KeyCode::Down => {
            if menu.selected + 1 < app.shell_profiles.len() {
                menu.selected += 1;
            }
        }
        KeyCode::Enter => {
            app.active_shell = menu.selected;
            app.mode = Mode::Browsing;
        }
        KeyCode::Esc => app.mode = Mode::Browsing,
        _ => {}
    }

    Ok(())
}
