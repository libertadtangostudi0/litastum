#[cfg(windows)]
mod alt_key;
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
    // Same isolation reasoning as command_history/search_history below
    // -- loaded here, not in App::new, so tests via test_support::test_app
    // never touch the real config.json for this.
    app.popup_style = theming::config::load_active_popup_style();
    // Loaded here rather than in `App::new` itself so every test that
    // builds an `App` (nearly all of them, via `test_support::test_app`)
    // stays isolated from whatever real `command_history.txt` happens
    // to sit in the test-running directory -- same reasoning as the
    // shell profile above not living inside `App::new` either.
    app.command_history = command_line::load_history();
    // Same isolation reasoning as command_history above -- loaded here,
    // not in App::new, so tests building an App via test_support::test_app
    // never touch a real editor_search_history.txt on disk.
    app.search_history = editor::find_history::load_history();
    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    // Unlike command_history.txt (saved incrementally, per command run,
    // from inside browsing::run_command_line), the search box has no
    // equivalent "needs a real Terminal" choke point to hang a disk
    // write off without also making handle_search_key's own extensive
    // unit tests touch the filesystem -- see editor_keymap.rs's own
    // comment on this. Saved once here instead, at clean exit; a crash
    // mid-session loses that session's own search history, same
    // tradeoff `command_history.txt` doesn't have to make, accepted for
    // keeping the key-handling tests filesystem-free.
    editor::find_history::save_history(&app.search_history);
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
        let mut layout = [(1usize, 0usize); 2];
        terminal.draw(|frame| layout = ui::draw(frame, app))?;
        for (panel, (cols, rows)) in app.panels.iter_mut().zip(layout) {
            panel.set_columns(cols);
            panel.set_visible_rows(rows);
        }
        wait_for_event(app, terminal)?;
    }
    Ok(())
}


/// Blocks until either a real terminal event arrives (dispatched via
/// `handle_event`) or -- Windows only -- the physical `Alt` key's
/// actual held state (`alt_key::is_physically_down`) changes, so the
/// alt-labels F-key row can react to `Alt` genuinely being held down,
/// not just to the next keypress that happens to carry the `Alt`
/// modifier. See `alt_key.rs`'s own doc for why that distinction
/// matters: `crossterm`'s Windows backend never emits an event for a
/// bare modifier key on its own, so relying on keypress modifiers
/// alone means the row only ever updates in the same frame an `Alt+`
/// shortcut already fired -- too late to be a preview. Elsewhere
/// (`cfg(not(windows))`), this just blocks on the next real event,
/// same as before; the keystroke-modifier approximation in
/// `handle_event` below is what drives `alt_held` there.
#[cfg(windows)]
fn wait_for_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    loop {
        if event::poll(alt_key::POLL_INTERVAL)? {
            return handle_event(app, terminal);
        }
        let alt_down = alt_key::is_physically_down();
        if alt_down != app.alt_held {
            app.alt_held = alt_down;
            return Ok(());
        }
    }
}

#[cfg(not(windows))]
fn wait_for_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    handle_event(app, terminal)
}


fn handle_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    // Drives the alternate F-key row (`ui::draw_function_keys`). On
    // Windows this is mostly redundant with `wait_for_event`'s own
    // `GetAsyncKeyState` poll (which already catches real hold/release
    // even between keystrokes) but harmless to also set here; on other
    // platforms this keystroke-modifier reading is the only signal
    // there is -- see `App::alt_held`'s doc.
    app.alt_held = key.modifiers.contains(KeyModifiers::ALT);

    match &app.mode {
        Mode::Editing(_) => editor::handle_editor_key(app, key),
        Mode::ConfirmDiscard(_) => editor::handle_confirm_discard_key(app, key),
        Mode::ConfirmDelete(_) => explorer::handle_confirm_delete_key(app, key),
        Mode::ConfirmTransfer(_) => explorer::handle_confirm_transfer_key(app, key),
        Mode::MainMenu(_) => theming::handle_main_menu_key(app, key),
        Mode::ThemeMenu(_) => theming::handle_theme_menu_key(app, key),
        Mode::ShellMenu(_) => command_line::handle_shell_menu_key(app, key),
        Mode::PopupStyleMenu(_) => theming::handle_popup_style_menu_key(app, key),
        Mode::FindFile(_) => explorer::handle_find_file_key(app, key),
        Mode::CommandHistory(_) => command_line::handle_history_key(app, key),
        Mode::ChangeDrive(_) => explorer::handle_drive_menu_key(app, key),
        Mode::Browsing => command_line::handle_browsing_key(app, key, terminal),
    }
}
