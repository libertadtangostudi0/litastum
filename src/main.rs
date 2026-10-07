mod app;
mod choice_menu;
mod command_line;
mod compare;
mod conflict;
mod editor;
mod event_loop;
mod explorer;
mod image_host;
mod app_data;
mod keyboard_layout;
mod list_cursor;
mod logging;
mod notice;
mod path_edit;
mod terminal_palette;
mod terminal_setup;
#[cfg(test)]
mod test_support;
mod text_field;
mod theming;
mod ui;
mod user_screen;
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
    // Queried after entering the alternate screen but before `run()` reads
    // events: the query uses raw escape sequences on stdio. On error,
    // `App::new`'s half-blocks stay. In litastum's own window there's
    // nothing to ask: it says what it draws (`image_host`).
    if let Some(cell) = image_host::host_cell_size() {
        app.image_picker = image_host::hosted_picker(cell);
    } else if let Ok(picker) = ratatui_image::picker::Picker::from_query_stdio() {
        app.image_picker = picker;
    }
    // The query wrote to the real screen behind `ratatui`'s buffer, and stray
    // text showed in a panel before the UI; `clear()` forces a full repaint.
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
    // A `FarMenu.ini` in the launch directory is offered for porting right
    // away, not only on `F2` (requested).
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

