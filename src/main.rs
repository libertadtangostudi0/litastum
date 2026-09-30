mod app;
mod choice_menu;
mod command_line;
mod compare;
mod editor;
mod event_loop;
mod explorer;
mod history_dir;
mod keyboard_layout;
mod list_cursor;
mod logging;
mod terminal_setup;
#[cfg(test)]
mod test_support;
mod text_field;
mod theming;
mod ui;
mod windows_terminal;
mod yes_no;

use color_eyre::eyre::Result;

use app::{App, Overlay};
use terminal_setup::{install_ctrl_c_handler, restore_terminal, setup_terminal};


fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    install_ctrl_c_handler()?;
    let start_dir = std::env::current_dir()?;

    let mut terminal = setup_terminal()?;
    let (theme, syntax_theme) = theming::config::load_active_theme();
    let mut app = App::new(start_dir, theme, syntax_theme)?;
    // Queried once here -- after entering the alternate screen
    // (`setup_terminal` above) but before `run()` starts reading
    // keyboard events -- see `App::image_picker`'s own doc comment for
    // why that ordering matters (the query writes/reads raw escape
    // sequences on stdio that could otherwise collide with, or be
    // swallowed by, crossterm's own event reader). Falls back to
    // `App::new`'s own `Picker::halfblocks()` default on any error,
    // same "never blocks startup" rule the rest of this function
    // follows.
    if let Ok(picker) = ratatui_image::picker::Picker::from_query_stdio() {
        app.image_picker = picker;
    }
    // The query above writes and reads raw escape sequences directly on
    // stdio, bypassing `ratatui`'s own render buffer entirely -- reported
    // directly as a real visible glitch (some stray text briefly shown
    // in a panel before the real UI appears). `ratatui`'s own diffing
    // render only rewrites cells that differ from its *own* last-known
    // buffer, which starts out blank and has no idea the query just
    // wrote real bytes to the actual screen -- so a query artifact
    // sitting outside whatever the very first frame happens to redraw
    // could otherwise linger. `Terminal::clear()` forces the next
    // `draw()` to treat the whole screen as needing a full repaint,
    // guaranteeing that first frame actually overwrites everything.
    terminal.clear()?;
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
    app.settings = theming::config::load_settings();
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
    // Same isolation reasoning again -- two separate files, one per
    // Find file field, see `explorer::find_file_history::NAME_HISTORY_FILE`/
    // `CONTENT_HISTORY_FILE`'s own doc comments.
    app.find_file_name_history = explorer::find_file_history::load_history(explorer::find_file_history::NAME_HISTORY_FILE);
    app.find_file_content_history = explorer::find_file_history::load_history(explorer::find_file_history::CONTENT_HISTORY_FILE);
    // One-time startup check, requested directly: a FarMenu.ini sitting
    // in the directory litastum was launched from should be offered for
    // porting immediately, not only once the user happens to press F2
    // there -- same Overlay::ConfirmPortFarMenu popup either way
    // (`explorer::user_menu::state::resolve_menu` reports a FarMenu.ini's
    // presence unconditionally, even over an already-configured
    // LitastumMenu.toml, so this also covers "I already have a menu and
    // just dropped a new FarMenu.ini in").
    if let explorer::MenuFile::FarMenuFound(far_path) = explorer::resolve_menu(&app.panels[0].path) {
        app.overlay = Some(Overlay::ConfirmPortFarMenu(far_path));
    }
    let result = event_loop::run(&mut terminal, &mut app);
    restore_terminal(&mut terminal, app.mouse_capture_enabled)?;
    // Unlike command_history.txt (saved per command, from
    // browsing::submit_command_line), the editor's search history is
    // saved only here: saving from handle_search_key would make its
    // unit tests touch the filesystem -- see editor_keymap.rs. Saved once here instead, at clean exit; a crash
    // mid-session loses that session's own search history, same
    // tradeoff `command_history.txt` doesn't have to make, accepted for
    // keeping the key-handling tests filesystem-free.
    editor::find_history::save_history(&app.search_history);
    // Same shape, same reasoning, for Find file's own two histories.
    explorer::find_file_history::save_history(explorer::find_file_history::NAME_HISTORY_FILE, &app.find_file_name_history);
    explorer::find_file_history::save_history(explorer::find_file_history::CONTENT_HISTORY_FILE, &app.find_file_content_history);
    result
}

