mod app;
mod command;
mod config;
mod editor;
mod editor_keymap;
mod keymap;
mod logging;
mod panel;
mod scheme;
mod theme;
mod theme_menu;
mod ui;

use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use app::{App, Mode};
use editor_keymap::EditorCommand;


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
        handle_event(app)?;
    }
    Ok(())
}


fn handle_event(app: &mut App) -> Result<()> {
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match &app.mode {
        Mode::Editing(_) => handle_editor_key(app, key),
        Mode::ConfirmDiscard(_) => handle_confirm_discard_key(app, key),
        Mode::ThemeMenu(_) => handle_theme_menu_key(app, key),
        Mode::Browsing => {
            debug!(?key, "browsing key");
            if let Some(cmd) = keymap::resolve(key.code) {
                debug!(?cmd, "browsing command");
                command::execute(cmd, app)?;
            }
            Ok(())
        }
    }
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
