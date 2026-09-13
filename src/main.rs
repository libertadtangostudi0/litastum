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
    event::{self, DisableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
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
    // One-time startup check, requested directly: a FarMenu.ini sitting
    // in the directory litastum was launched from should be offered for
    // porting immediately, not only once the user happens to press F2
    // there -- same Mode::ConfirmPortFarMenu popup either way
    // (`explorer::user_menu::state::resolve_menu` reports a FarMenu.ini's
    // presence unconditionally, even over an already-configured
    // LitastumMenu.toml, so this also covers "I already have a menu and
    // just dropped a new FarMenu.ini in").
    if let explorer::MenuFile::FarMenuFound(far_path) = explorer::resolve_menu(&app.panels[0].path) {
        app.mode = Mode::ConfirmPortFarMenu(far_path);
    }
    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal, app.mouse_capture_enabled)?;
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


fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mouse_capture_enabled: bool) -> Result<()> {
    disable_raw_mode()?;
    // `DisableMouseCapture` here is a safety net, not the primary
    // toggle -- mouse capture is normally turned on/off around just the
    // Markdown preview session itself
    // (`explorer::markdown_preview::open_preview`/
    // `handle_markdown_preview_key`), so it doesn't interfere with
    // native mouse text-selection everywhere else in this app. But
    // quitting (`F10`) while a preview happens to still be open would
    // otherwise skip that "turn it back off" step and leak mouse
    // capture into the user's terminal after this process exits.
    //
    // `mouse_capture_enabled` (`app.mouse_capture_enabled`) gates
    // whether `DisableMouseCapture` is even attempted -- reported as a
    // real crash on Windows (`Error: 0: Initial console modes not set`)
    // from sending it *unconditionally*: `crossterm`'s Windows console
    // backend has no "initial mode" saved to restore unless
    // `EnableMouseCapture` actually ran first in this process, and
    // quitting from a plain `Mode::Browsing` session (never having
    // opened a Markdown preview at all) hit exactly that case.
    if mouse_capture_enabled {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape
    )?;
    Ok(())
}


fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    // Seeds `app.panels`' own `columns`/`visible_rows` from the real
    // terminal size before the loop's *own* first frame ever reaches the
    // screen -- `Panel::new()` defaults both to a single-column
    // placeholder (`columns: 1`, `visible_rows: 0`, `column_height()`'s
    // own doc comment calls this the "not yet known" sentinel), and the
    // real values computed by `ui::draw` only ever get fed back to
    // *this* draw's own returned `layout` -- applied to `app.panels`
    // only *after* the frame that used the old, wrong values has
    // already been sent to the terminal. Reported directly as a real,
    // persistent glitch (not just an imperceptible one-frame flash):
    // every entry crammed into one narrow column, staying that way
    // until the very next keypress forced a redraw, since
    // `wait_for_event` below blocks for input in between. This extra
    // draw+apply cycle up front means the loop's own first visible frame
    // already has correct, real values to render with.
    let mut layout = [(1usize, 0usize); 2];
    terminal.draw(|frame| layout = ui::draw(frame, app))?;
    for (panel, (cols, rows)) in app.panels.iter_mut().zip(layout) {
        panel.set_columns(cols);
        panel.set_visible_rows(rows);
    }

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
    match event::read()? {
        Event::Key(key) => handle_key_event(app, key, terminal),
        // Only ever arrives while `App::markdown_edit_preview` has
        // turned capture on for itself
        // (`explorer::markdown_preview::open_edit_preview`'s own doc
        // comment) -- every other mode just never gets a mouse event to
        // begin with, so no mode check is needed here the way
        // `handle_key_event`'s own big match needs one per mode.
        // `handle_markdown_preview_mouse` itself no-ops outside
        // `Mode::Editing`/without a linked preview.
        Event::Mouse(mouse) => {
            explorer::handle_markdown_preview_mouse(app, mouse);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
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
        // `Tab` toggles which half of a combined editor+preview session
        // (`App::markdown_edit_preview`) keyboard input reaches -- `0`
        // the editor, `1` the embedded preview -- intercepted here,
        // ahead of both `editor::handle_editor_key` and
        // `explorer::handle_markdown_edit_preview_key`, since it's
        // meaningless to either on its own (plain `F4` editing, with no
        // linked preview, forwards `Tab` straight to the editor as
        // always -- see the `_ if` guard's own condition).
        Mode::Editing(_) if app.markdown_edit_preview.is_some() && key.code == KeyCode::Tab => {
            app.active = 1 - app.active;
            Ok(())
        }
        Mode::Editing(_) if app.markdown_edit_preview.is_some() && app.active == 1 => explorer::handle_markdown_edit_preview_key(app, key),
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
        Mode::UserMenu(_) => explorer::handle_user_menu_key(app, key, terminal),
        Mode::UserMenuPrompt(_) => explorer::handle_user_menu_prompt_key(app, key, terminal),
        Mode::ConfirmPortFarMenu(_) => explorer::handle_confirm_port_far_menu_key(app, key),
        Mode::AddUserMenuItem(..) => explorer::handle_add_user_menu_item_key(app, key),
        Mode::ImagePreview(_) => {
            explorer::handle_image_preview_key(app, key);
            Ok(())
        }
        Mode::MarkdownLinkSearch(..) => {
            explorer::handle_markdown_link_search_key(app, key);
            Ok(())
        }
        // Any key dismisses -- there's nothing to answer, just
        // something to acknowledge having read.
        Mode::Info(_) => {
            app.mode = Mode::Browsing;
            Ok(())
        }
        Mode::Browsing => command_line::handle_browsing_key(app, key, terminal),
    }
}
