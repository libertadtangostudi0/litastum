mod app;
mod command;
mod editor;
mod editor_keymap;
mod keymap;
mod logging;
mod panel;
mod theme;
mod ui;

use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use app::{App, Mode};
use editor_keymap::EditorCommand;
use theme::Theme;


fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let start_dir = std::env::current_dir()?;

    let mut terminal = setup_terminal()?;
    let mut app = App::new(start_dir)?;
    let theme = Theme::dark();
    let result = run(&mut terminal, &mut app, &theme);
    restore_terminal(&mut terminal)?;
    result
}


fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}


fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}


fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App, theme: &Theme) -> Result<()> {
    while !app.should_quit {
        let mut columns = [1usize; 2];
        terminal.draw(|frame| columns = ui::draw(frame, app, theme))?;
        for (panel, cols) in app.panels.iter_mut().zip(columns) {
            panel.set_columns(cols);
        }
        handle_event(app, theme)?;
    }
    Ok(())
}


fn handle_event(app: &mut App, theme: &Theme) -> Result<()> {
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match &app.mode {
        Mode::Editing(_) => handle_editor_key(app, key),
        Mode::ConfirmDiscard(_) => handle_confirm_discard_key(app, key),
        Mode::Browsing => {
            debug!(?key, "browsing key");
            if let Some(cmd) = keymap::resolve(key.code) {
                debug!(?cmd, "browsing command");
                command::execute(cmd, app, theme)?;
            }
            Ok(())
        }
    }
}


/// Key handling while a file is open in the built-in editor. Resolution
/// (`editor_keymap::resolve`) and execution are split the same way as
/// the browsing-mode `keymap`/`command` pair, for the same reason: the
/// key table only grows from here, and resolution alone is unit
/// testable without touching `Editor`/the OS clipboard.
fn handle_editor_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let command = editor_keymap::resolve(key);
    debug!(?key, ?command, "editor key");

    if command == EditorCommand::Close {
        return close_editor_or_confirm(app);
    }

    let Mode::Editing(active_editor) = &mut app.mode else {
        return Ok(());
    };

    match command {
        EditorCommand::Close => unreachable!("handled above"),
        EditorCommand::Save => active_editor.save()?,
        EditorCommand::Copy => active_editor.copy(),
        EditorCommand::Cut => active_editor.cut(),
        EditorCommand::Paste => active_editor.paste(),
        EditorCommand::Input => active_editor.input(key),
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
