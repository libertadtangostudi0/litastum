mod app;
mod command_line;
mod editor;
mod explorer;
mod logging;
#[cfg(test)]
mod test_support;
mod text_field;
mod theming;
mod ui;

use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};

use app::{App, Mode};


fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    let start_dir = std::env::current_dir()?;

    let mut terminal = setup_terminal()?;
    let (theme, syntax_theme) = theming::config::load_active_theme();
    let mut app = App::new(start_dir, theme, syntax_theme)?;
    // Applies a shell profile saved via F9 -> Options -> Save setup, if
    // its name still matches one of the built-in profiles -- a name
    // that no longer exists (profiles changed between runs) just falls
    // back to the default at index 0, same as a broken theme choice
    // falls back rather than failing startup.
    if let Some(name) = theming::config::load_active_shell() {
        if let Some(index) = app.shell_profiles.iter().position(|profile| profile.name == name) {
            app.active_shell = index;
        }
    }
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

    // Drives the alternate F-key row (`ui::draw_function_keys`) -- see
    // `App::alt_held`'s doc for why this is "did the last key event
    // carry Alt" rather than true hold/release tracking.
    app.alt_held = key.modifiers.contains(KeyModifiers::ALT);

    match &app.mode {
        Mode::Editing(_) => editor::handle_editor_key(app, key),
        Mode::ConfirmDiscard(_) => editor::handle_confirm_discard_key(app, key),
        Mode::ConfirmDelete(_) => explorer::handle_confirm_delete_key(app, key),
        Mode::ConfirmTransfer(_) => explorer::handle_confirm_transfer_key(app, key),
        Mode::MainMenu(_) => theming::handle_main_menu_key(app, key),
        Mode::ThemeMenu(_) => theming::handle_theme_menu_key(app, key),
        Mode::ShellMenu(_) => command_line::handle_shell_menu_key(app, key),
        Mode::FindFile(_) => explorer::handle_find_file_key(app, key),
        Mode::CommandHistory(_) => command_line::handle_history_key(app, key),
        Mode::Browsing => command_line::handle_browsing_key(app, key, terminal),
    }
}
