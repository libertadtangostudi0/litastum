mod app;
mod panel;
mod ui;

use std::io::{self, Stdout};
use std::process::Command;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};

use app::App;


fn main() -> Result<()> {
    color_eyre::install()?;
    let start_dir = std::env::current_dir()?;

    let mut terminal = setup_terminal()?;
    let mut app = App::new(start_dir)?;
    let result = run(&mut terminal, &mut app);
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


fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        handle_event(terminal, app)?;
    }
    Ok(())
}


fn handle_event(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match key.code {
        KeyCode::Up => app.active_panel().move_up(),
        KeyCode::Down => app.active_panel().move_down(),
        KeyCode::Enter => app.active_panel().enter_selected()?,
        KeyCode::Tab => app.toggle_active(),
        KeyCode::F(4) => open_editor_for_selection(terminal, app)?,
        KeyCode::F(10) => app.should_quit = true,
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }

    Ok(())
}


/// Suspends the TUI, opens the file under the cursor in an external
/// editor, then restores the TUI and reloads the panel's contents.
///
/// This is the deliberately minimal "shell-out" F4 from stage 2 of the
/// roadmap; a built-in editor widget (ratatui-textarea / edtui) replaces
/// this in a later stage.
fn open_editor_for_selection(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let Some(path) = app.active_panel().selected_path() else {
        return Ok(());
    };
    if path.is_dir() {
        return Ok(());
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| default_editor());

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let status = Command::new(&editor).arg(&path).status();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    if let Err(err) = status {
        eprintln!("Failed to launch editor '{editor}': {err}");
    }

    app.active_panel().reload()?;
    Ok(())
}


#[cfg(windows)]
fn default_editor() -> String {
    "notepad".to_string()
}


#[cfg(not(windows))]
fn default_editor() -> String {
    "nano".to_string()
}
