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
    event::{self, DisableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
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
    app.editor_keymap_mode = theming::config::load_active_editor_keymap_mode();
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


/// How often `wait_for_event` checks a background image decode
/// (`explorer::is_image_decode_pending`/`poll_pending_image_decode`)
/// while one is in flight -- `F3`'s image preview moved its own
/// decode/resize off the main thread after both the very first open
/// and every `Left`/`Right` switch were reported as blocking the whole
/// UI for however long that took (`ImagePreviewState`'s own doc
/// comment has the full story). Short enough that a finished decode
/// appears essentially instantly once ready, without needing a real
/// keyboard/mouse event to happen to wake the loop up and notice it.
const IMAGE_DECODE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(30);

/// Blocks until either a real terminal event arrives (dispatched via
/// `handle_event`), a pending background image decode finishes
/// (`IMAGE_DECODE_POLL_INTERVAL`, above), or -- Windows only -- the
/// physical `Alt` key's actual held state (`alt_key::is_physically_down`)
/// changes, so the alt-labels F-key row can react to `Alt` genuinely
/// being held down, not just to the next keypress that happens to carry
/// the `Alt` modifier. See `alt_key.rs`'s own doc for why that
/// distinction matters: `crossterm`'s Windows backend never emits an
/// event for a bare modifier key on its own, so relying on keypress
/// modifiers alone means the row only ever updates in the same frame an
/// `Alt+` shortcut already fired -- too late to be a preview. Elsewhere
/// (`cfg(not(windows))`), this still just blocks on the next real event
/// when nothing's pending, same as before either of these polling
/// reasons existed; the keystroke-modifier approximation in
/// `handle_event` below is what drives `alt_held` there.
#[cfg(windows)]
fn wait_for_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    loop {
        let poll_interval = if explorer::is_image_decode_pending(app) { IMAGE_DECODE_POLL_INTERVAL } else { alt_key::POLL_INTERVAL };
        if event::poll(poll_interval)? {
            return handle_event(app, terminal);
        }
        if explorer::poll_pending_image_decode(app) {
            return Ok(());
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
    if !explorer::is_image_decode_pending(app) {
        return handle_event(app, terminal);
    }
    loop {
        if event::poll(IMAGE_DECODE_POLL_INTERVAL)? {
            return handle_event(app, terminal);
        }
        if explorer::poll_pending_image_decode(app) {
            return Ok(());
        }
    }
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
            let last_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown).then_some(mouse.kind);
            drain_pending_mouse_events(app, terminal, last_scroll)
        }
        _ => Ok(()),
    }
}

/// Processes every further event already sitting in `crossterm`'s own
/// queue, without blocking (`event::poll` with a zero timeout), for as
/// long as they keep being mouse events -- reported directly as a real
/// lag, response to touchpad/mouse-wheel scrolling sometimes taking a
/// very long time to catch up. A touchpad (and some mice) deliver
/// one scroll gesture as a rapid burst of many small discrete tick
/// events rather than a single one, but `run()`'s own loop does a full
/// `terminal.draw()` after *every* event `handle_event` returns from --
/// redrawing between each individual tick of a fast scroll means the
/// UI visibly falls behind the gesture, worse the larger a single
/// frame's own render cost is (word-wrapping a Markdown preview, `edtui`'s
/// own per-frame syntax highlighting, ...). Draining the whole burst
/// here and letting `run()` draw exactly once afterward fixes that
/// without touching `handle_markdown_preview_mouse` itself at all -- the
/// backlog was in how often a frame got drawn, not in how any single
/// event was handled.
///
/// Drains until the queue is genuinely empty, not capped to a fixed
/// time budget -- an earlier version added a one-frame (16ms) cap
/// specifically so a direction reversal couldn't be hidden behind an
/// unbounded backlog, but capping it that way meant a *long*,
/// uninterrupted scroll now redrew roughly 60 times a second even
/// though nothing but the scroll position itself was changing each
/// time -- reported directly as feeling slower than the uncapped
/// version that preceded it. `last_scroll` below already handles the
/// actual reversal case directly (see its own doc), so the time cap
/// wasn't buying anything the reversal check didn't already cover --
/// removed rather than tuned smaller. The tradeoff this accepts,
/// deliberately: a long same-direction scroll no longer redraws
/// incrementally while it's still in motion, only once the whole burst
/// has actually drained -- exactly what was asked for (no need to
/// track the cursor position mid-scroll, it'll show up at the top of
/// the view once scrolling stops), trading mid-scroll visual
/// feedback for fewer total redraws.
///
/// `last_scroll`: the moment a drained event's own scroll direction
/// actually differs from the previous one, this returns immediately
/// after handling it, rather than continuing to drain. A real
/// direction change is exactly the one case where continuing to
/// coalesce is actively wrong -- it's not "more of the same gesture"
/// to merge, it's the next gesture already starting. Plain clicks and
/// non-scroll mouse events don't update `last_scroll` at all -- only
/// an actual direction *change* between two scrolls should cut the
/// drain short.
///
/// A key event found mid-burst is still handled (never silently
/// dropped) -- it just ends the drain there, matching every other call
/// site's own "one real event per `wait_for_event` call" convention.
fn drain_pending_mouse_events(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, mut last_scroll: Option<MouseEventKind>) -> Result<()> {
    while event::poll(std::time::Duration::from_secs(0))? {
        match event::read()? {
            Event::Mouse(mouse) => {
                let is_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
                let reversed = is_scroll && last_scroll.is_some_and(|prev| prev != mouse.kind);
                explorer::handle_markdown_preview_mouse(app, mouse);
                if reversed {
                    return Ok(());
                }
                if is_scroll {
                    last_scroll = Some(mouse.kind);
                }
            }
            Event::Key(key) => return handle_key_event(app, key, terminal),
            _ => {}
        }
    }
    Ok(())
}

/// `Up`/`Down`/`PageUp`/`PageDown`, held down, generate a rapid burst of
/// distinct `KeyEventKind::Press` events via the OS's own key-repeat --
/// `crossterm`'s Windows backend never reports a separate "repeat" kind
/// distinguishing these from a fresh press (unlike some Unix terminals),
/// so each one looks like an ordinary keystroke and gets its own full
/// dispatch. These four are the ones actually meant to be held for a
/// stretch to move through a long list/document/buffer, the exact same
/// shape of problem `drain_pending_mouse_events` already fixed for a
/// touchpad's own scroll-wheel burst -- reported directly against the
/// built-in editor's own text caret specifically (it kept traveling
/// past where the user stopped scrolling, and reversing direction took
/// too long to catch up), but the same backlog can
/// build up navigating a long file panel listing or popup list too,
/// since `dispatch_key_event`'s own big match is one shared chokepoint
/// for all of them.
fn is_repeatable_navigation_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown)
}

/// Whether `next` reverses the axis `prev` was moving on -- `Up`/`Down`
/// undo each other, `PageUp`/`PageDown` undo each other, but a
/// same-key repeat or a switch between the two axes isn't a reversal at
/// all (there's no "held down, then immediately Page Up" gesture this
/// needs to special-case the way a scroll wheel's own two-directions-only
/// axis does).
fn is_navigation_reversal(prev: KeyCode, next: KeyCode) -> bool {
    matches!((prev, next), (KeyCode::Up, KeyCode::Down) | (KeyCode::Down, KeyCode::Up) | (KeyCode::PageUp, KeyCode::PageDown) | (KeyCode::PageDown, KeyCode::PageUp))
}

/// Dispatches `key`, then -- only when it was one of the four
/// held-to-scroll keys above -- drains any further same-axis repeats
/// already queued up, redrawing once for the whole burst instead of
/// once per repeat, exactly mirroring `drain_pending_mouse_events`'s
/// own reasoning and its immediate-stop-on-reversal behavior. Every
/// other key (typing, `Left`/`Right`, `Enter`, `Esc`, ...) is completely
/// unaffected -- dispatched once, same as always, since those don't
/// have this repeat-burst shape (`Left`/`Right` move by a single
/// character/column, not a whole page, so a repeat backlog there is
/// nowhere near long enough to notice, and coalescing typed characters
/// would be actively wrong).
fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    dispatch_key_event(app, key, terminal)?;
    if key.kind == KeyEventKind::Press && is_repeatable_navigation_key(key.code) {
        drain_pending_navigation_keys(app, terminal, key.code)?;
    }
    Ok(())
}

/// See `handle_key_event`'s own doc comment. A key found mid-burst that
/// isn't one of the four repeatable navigation keys (or a mouse event)
/// is still fully dispatched -- it just ends this drain, same "never
/// silently drop an event, just stop coalescing" rule
/// `drain_pending_mouse_events` already follows.
fn drain_pending_navigation_keys(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, mut last: KeyCode) -> Result<()> {
    while event::poll(std::time::Duration::from_secs(0))? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press && is_repeatable_navigation_key(key.code) => {
                let reversed = is_navigation_reversal(last, key.code);
                dispatch_key_event(app, key, terminal)?;
                if reversed {
                    return Ok(());
                }
                last = key.code;
            }
            Event::Key(key) => return dispatch_key_event(app, key, terminal),
            Event::Mouse(mouse) => {
                explorer::handle_markdown_preview_mouse(app, mouse);
                let last_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown).then_some(mouse.kind);
                return drain_pending_mouse_events(app, terminal, last_scroll);
            }
            _ => {}
        }
    }
    Ok(())
}

fn dispatch_key_event(app: &mut App, key: crossterm::event::KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
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
        Mode::EditorMenu(_, _) => editor::handle_editor_menu_key(app, key),
        Mode::EditorKeymapMenu(_, _) => editor::handle_editor_keymap_menu_key(app, key),
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
