#[cfg(windows)]
mod alt_key;
mod app;
mod command_line;
mod compare;
mod editor;
mod explorer;
mod history_dir;
mod keyboard_layout;
mod list_cursor;
mod logging;
#[cfg(windows)]
mod paste_hotkey;
#[cfg(test)]
mod test_support;
mod text_field;
mod theming;
mod ui;

use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use app::{App, Mode};


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
    app.popup_style = theming::config::load_active_popup_style();
    app.editor_keymap_mode = theming::config::load_active_editor_keymap_mode();
    app.compare_line_ending_display = theming::config::load_active_compare_line_ending_display();
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
    // Same shape, same reasoning, for Find file's own two histories.
    explorer::find_file_history::save_history(explorer::find_file_history::NAME_HISTORY_FILE, &app.find_file_name_history);
    explorer::find_file_history::save_history(explorer::find_file_history::CONTENT_HISTORY_FILE, &app.find_file_content_history);
    result
}


/// Reported directly: `Ctrl+C` pressed while a shelled-out command was
/// running (`command_line::run_shell_command_lines` /
/// `toggle_panels_hidden`'s own console loop -- both leave raw mode and
/// hand the console to a real child process) took the whole litastum
/// process down with it, not just the child. Root cause: raw mode is
/// what normally keeps `Ctrl+C` from ever becoming an OS-level signal
/// at all -- `enable_raw_mode` clears `ENABLE_PROCESSED_INPUT` on
/// Windows / `ISIG` on Unix, so while the TUI itself has raw mode on,
/// `Ctrl+C` arrives as an ordinary `KeyEvent` like any other key, never
/// a signal. The moment raw mode is turned back off to run a real
/// subprocess with inherited stdio, that protection goes away too --
/// `Ctrl+C` becomes a real `CTRL_C_EVENT`/`SIGINT` again, delivered to
/// *every* process still attached to the same console/process group,
/// our own included. With no handler of our own installed, the
/// platform's default action for that signal is to terminate the
/// process -- so litastum died right along with the child it was
/// waiting on.
///
/// Installing an otherwise-empty handler here doesn't suppress the
/// signal for the *child* -- it's delivered to that process
/// independently, and an ordinary CLI tool's own default disposition
/// (interrupt/exit) still applies to it exactly as if it had been run
/// directly in a real shell. It only stops *our* process from dying
/// alongside it: the child gets interrupted, `Command::status()`
/// returns once it exits, and control comes back to whichever of our
/// own wrapper functions was waiting on it -- the same "Ctrl+C
/// interrupts the foreground command, not the shell itself" behavior
/// every real shell already has. `ctrlc` (rather than hand-rolling
/// `SetConsoleCtrlHandler`/`sigaction` behind `#[cfg(windows)]`/
/// `#[cfg(unix)]` ourselves) covers both platforms' actual mechanism
/// with one call.
fn install_ctrl_c_handler() -> Result<()> {
    ctrlc::set_handler(|| debug!("Ctrl+C received; ignored at the litastum process level"))?;
    Ok(())
}


fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // A thin bar matches a normal text caret; edtui's own cursor
    // highlight is turned off in editor.rs so this is what's visible
    // while editing (blinking, so it's still findable at a glance).
    //
    // `EnableBracketedPaste` -- reported directly as a real, severe
    // performance problem: pasting a multi-hundred-line file into the
    // built-in editor "felt like watching it render line by line" and
    // took well over a minute, even after `editor::fast_paste_from_clipboard`
    // made the actual splice itself sub-millisecond
    // (`editor/editor/fast_paste.rs`). Root cause was one level further
    // out than the buffer-insert algorithm: without bracketed-paste
    // mode, the terminal has no way to tell this app "this whole block
    // arrived from a paste, not a human typing" -- it just feeds every
    // character of the pasted text through as its own separate
    // `Event::Key` press. Each one is a real keystroke as far as this
    // app is concerned, hitting the *normal* per-character `InsertChar`
    // path (never `fast_paste_from_clipboard`'s own `Ctrl+V` interception
    // at all, since there's no `Ctrl+V` keypress anywhere in this
    // stream to intercept) and triggering `main.rs::run`'s own full
    // per-event redraw every single time -- thousands of characters,
    // thousands of redraws, is exactly what "line by line" rendering
    // looks like from the outside. `EnableBracketedPaste` asks the
    // terminal to instead wrap a paste in `ESC[200~.../ESC[201~` and
    // hand it to `crossterm` as one single `Event::Paste(String)` --
    // handled in `handle_event`, below.
    execute!(stdout, EnterAlternateScreen, SetCursorStyle::BlinkingBar, EnableBracketedPaste)?;
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
        DisableBracketedPaste,
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape
    )?;
    Ok(())
}


/// Draws one frame and applies whatever real terminal cursor position
/// `ui::draw` returned -- factored out since both the loop below and
/// its own up-front priming draw (see its doc comment) need the exact
/// same sequence.
///
/// Applies the cursor *after* `terminal.draw` has actually finished and
/// flushed, in `set_cursor_position` then `show_cursor` order -- the
/// reverse of what `ratatui::Terminal::draw` would have done on its own
/// had `ui::draw` still called `Frame::set_cursor_position` internally.
/// See `ui::draw`'s own doc comment for the full trace through
/// `ratatui`'s and `ratatui-crossterm`'s source confirming why that
/// order (`show_cursor` before the real target position is applied)
/// is what caused a real, reported flicker -- moving to *this* order
/// means the cursor only ever becomes visible already sitting in the
/// right place, for every `Mode` that places one, not just the command
/// line.
fn draw_and_apply_cursor(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<[(usize, usize); 2]> {
    let mut layout = [(1usize, 0usize); 2];
    let mut cursor = None;
    terminal.draw(|frame| {
        let (drawn_layout, drawn_cursor) = ui::draw(frame, app);
        layout = drawn_layout;
        cursor = drawn_cursor;
    })?;
    match cursor {
        Some(position) => {
            terminal.set_cursor_position(position)?;
            terminal.show_cursor()?;
        }
        None => terminal.hide_cursor()?,
    }
    Ok(layout)
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
    let layout = draw_and_apply_cursor(terminal, app)?;
    for (panel, (cols, rows)) in app.panels.iter_mut().zip(layout) {
        panel.set_columns(cols);
        panel.set_visible_rows(rows);
    }

    while !app.should_quit {
        let layout = draw_and_apply_cursor(terminal, app)?;
        for (panel, (cols, rows)) in app.panels.iter_mut().zip(layout) {
            panel.set_columns(cols);
            panel.set_visible_rows(rows);
        }
        wait_for_event(app, terminal)?;
    }
    Ok(())
}


/// How often `wait_for_event` checks a background image decode or Find
/// file search (`background_task_pending`/`poll_background_tasks`
/// below) while one is in flight -- `F3`'s image preview moved its own
/// decode/resize off the main thread after both the very first open
/// and every `Left`/`Right` switch were reported as blocking the whole
/// UI for however long that took (`ImagePreviewState`'s own doc
/// comment has the full story); the Find file search
/// (`explorer::find_file::background`) joined it later for the same
/// reason, once its own real result was starting to take long enough
/// to matter. Short enough that a finished decode or search appears
/// essentially instantly once ready, without needing a real keyboard/
/// mouse event to happen to wake the loop up and notice it.
const BACKGROUND_TASK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(30);

/// Whether any background task `wait_for_event` should be polling for
/// is currently in flight -- an image decode, or a Find file search.
/// Kept as one shared check (rather than two separate `if`s repeated at
/// every call site below) since both `#[cfg]` variants of
/// `wait_for_event` need the exact same OR of the two.
fn background_task_pending(app: &App) -> bool {
    explorer::is_image_decode_pending(app) || explorer::is_find_file_search_pending(app)
}

/// Polls every background task once, applying whichever one (if any)
/// has actually finished -- `true` means something changed and the
/// caller should redraw. Both polls always run (not short-circuited),
/// since an image decode and a Find file search can't currently be in
/// flight at the same time in this app anyway (`Mode` is one variant at
/// once), but there's no reason to make that assumption load-bearing
/// here.
fn poll_background_tasks(app: &mut App) -> bool {
    let image = explorer::poll_pending_image_decode(app);
    let search = explorer::poll_pending_find_file_search(app);
    image || search
}

/// Blocks until either a real terminal event arrives (dispatched via
/// `handle_event`), a pending background task finishes
/// (`BACKGROUND_TASK_POLL_INTERVAL`, above), or -- Windows only -- the
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
        // Checked every iteration, not just once per call the way the
        // idle-only `alt_down` check below works -- during an active
        // Windows-Terminal-injected paste flood, this loop keeps
        // finding a real `crossterm` event (one flooded character)
        // almost every time, so an idle-only check would never run
        // until the flood was already over. See `paste_hotkey.rs`'s own
        // module doc comment for the full story.
        if try_intercept_paste_hotkey(app)? {
            return Ok(());
        }
        let poll_interval = if background_task_pending(app) { BACKGROUND_TASK_POLL_INTERVAL } else { alt_key::POLL_INTERVAL };
        if event::poll(poll_interval)? {
            // A key event `handle_event` never actually dispatched
            // anything for (see `handle_key_event`'s own doc comment)
            // keeps this loop going instead of returning -- there's
            // nothing new on screen to justify `run()`'s own redraw.
            if handle_event(app, terminal)? {
                return Ok(());
            }
            continue;
        }
        if poll_background_tasks(app) {
            return Ok(());
        }
        let alt_down = alt_key::is_physically_down();
        if alt_down != app.alt_held {
            app.alt_held = alt_down;
            return Ok(());
        }
    }
}

/// Edge-triggered: fires the fast paste path once per distinct real
/// physical `Ctrl+V` press (`app.ctrl_v_physically_held` tracks the
/// previous poll's state, same pattern `wait_for_event`'s own
/// `alt_held` tracking already uses), never repeatedly while the combo
/// stays held. Returns `true` (asking `wait_for_event` to return and
/// let `run()` redraw) only when something actually happened --a fresh
/// press outside the one context this has a fast path for (the
/// built-in editor, in ordinary typing mode -- `Editor::is_plain_standard_typing`)
/// is deliberately left completely alone, no swallow armed, so
/// `crossterm`'s normal (slow) event flow handles it exactly as before
/// anywhere else -- extending this to the command line/other popups is
/// real future work, not something this fix reaches for speculatively.
///
/// See `paste_hotkey.rs`'s own module doc comment for why a real
/// terminal `Ctrl+V` needs this bypass in the first place, and for how
/// the mismatch between "read the clipboard right now" and "Windows
/// Terminal's own flood is still coming, one keystroke at a time" is
/// resolved: `app.pending_paste_swallow` is armed here (to the pasted
/// text's own keystroke-equivalent length) so `handle_key_event` can
/// silently discard that flood once it actually arrives, instead of
/// typing the same text a second time right after this already pasted
/// it once, instantly.
#[cfg(windows)]
fn try_intercept_paste_hotkey(app: &mut App) -> Result<bool> {
    let held = paste_hotkey::ctrl_v_physically_down();
    let just_pressed = held && !app.ctrl_v_physically_held;
    app.ctrl_v_physically_held = held;
    if !just_pressed {
        return Ok(false);
    }

    let Mode::Editing(editor) = &mut app.mode else {
        return Ok(false);
    };
    if !editor.is_plain_standard_typing() {
        return Ok(false);
    }
    let text = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        Ok(text) => text,
        Err(_) => return Ok(false),
    };
    if text.is_empty() {
        return Ok(false);
    }

    editor.paste_text(&text);
    arm_paste_swallow(app, &text);
    Ok(true)
}

/// Sets up `app.pending_paste_swallow`/`_deadline` right after a fast
/// paste (`try_intercept_paste_hotkey`) so `handle_key_event` knows what
/// Windows Terminal's own still-incoming keystroke flood is expected to
/// look like, in order, and can silently discard it -- see
/// `App::pending_paste_swallow`'s own doc comment for the queue itself,
/// and `should_swallow_paste_tail`'s for why content-matching (not just
/// counting) is what actually fixes real typing getting stuck behind a
/// still-draining swallow window.
///
/// **Extends the existing queue, never overwrites it.** Real reported
/// bug: pasting again quickly (before the *first* paste's own flood had
/// finished arriving) left a visible, slow, character-by-character
/// typing delay afterward -- overwriting `pending_paste_swallow` with
/// just the second paste's own text threw away whatever was left of
/// the first paste's still-incoming flood, so those leftover characters
/// no longer matched anything expected and got typed as real (if
/// nonsensical) input, one throttled keystroke at a time, right where
/// `should_swallow_paste_tail`'s own "an expired deadline or a mismatch
/// ends the swallow" rule tries to protect *genuine* typing.
/// Appending instead means a still-pending tail from an earlier paste
/// keeps getting silently discarded first, in the same order the two
/// floods should actually arrive in (Windows Terminal processes one
/// paste's own injection before starting the next), with the new
/// paste's own expected characters queued up right after it.
///
/// `\r`-stripped -- a `\n` in the pasted text becomes one `Enter`
/// keystroke, everything else becomes one `Char` keystroke, but a `\r`
/// immediately before a `\n` (Windows-style line endings) never becomes
/// a keystroke of its own at all, matching
/// `editor::fast_paste::splice_paste`'s own normalization.
#[cfg(windows)]
fn arm_paste_swallow(app: &mut App, text: &str) {
    app.pending_paste_swallow.extend(text.chars().filter(|&c| c != '\r'));
    // Generous relative to the ~7-8ms/char rate this was actually
    // measured at (`logs/litastum.log`, see `paste_hotkey.rs`'s own
    // doc comment) -- this only exists to eventually give up if the
    // flood never arrives, not to race it. Recomputed from the whole
    // (possibly just-extended) queue, not just this call's own text, so
    // a second paste's own budget still covers whatever's left of an
    // earlier one queued ahead of it.
    let budget_ms = (app.pending_paste_swallow.len() as u64).saturating_mul(100).max(2000);
    app.pending_paste_swallow_deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(budget_ms));
}

#[cfg(not(windows))]
fn wait_for_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    loop {
        if !background_task_pending(app) {
            if handle_event(app, terminal)? {
                return Ok(());
            }
            continue;
        }
        if event::poll(BACKGROUND_TASK_POLL_INTERVAL)? {
            if handle_event(app, terminal)? {
                return Ok(());
            }
            continue;
        }
        if poll_background_tasks(app) {
            return Ok(());
        }
    }
}


/// Returns whether the caller (`wait_for_event`) should actually redraw
/// -- `false` only for a key event that `handle_key_event` itself never
/// acted on at all (see its own doc comment), so `run()`'s own loop
/// doesn't spend a full frame redrawing something nothing changed.
fn handle_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<bool> {
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
            drain_pending_mouse_events(app, terminal, last_scroll)?;
            Ok(true)
        }
        Event::Paste(text) => {
            handle_paste_event(app, terminal, &text)?;
            Ok(true)
        }
        _ => Ok(true),
    }
}

/// A bracketed paste (`EnableBracketedPaste` in `setup_terminal`) --
/// see its own doc comment for the real reported bug this exists to
/// fix. While the built-in editor is open, hands `text` straight to
/// `Editor::paste_text` -- the same fast, O(text length) splice
/// `Ctrl+V` itself uses (`editor::fast_paste_from_clipboard`), just fed
/// from this event's own text instead of a fresh clipboard read (the
/// terminal already handed it to us; reading the clipboard again would
/// just be redundant, and could even race a clipboard change between
/// the copy and this paste actually arriving).
///
/// Every other mode has no equivalent fast path of its own (the
/// always-live command line, Find file's fields, the transfer popup,
/// ...) -- replayed as ordinary per-character key presses through the
/// exact same `dispatch_key_event` a real keystroke would take, just
/// looped here with no redraw in between rather than arriving one at a
/// time over the wire with a full redraw after each (which is what
/// "no bracketed paste" looked like before this existed, for *every*
/// mode, not just the editor). A newline in the pasted text is skipped
/// rather than replayed as `Enter` -- these fields are single-line, and
/// forwarding it could submit a command/form the user never meant to
/// trigger this instant, the same "may execute unexpected commands"
/// concern Windows Terminal's own multi-line-paste warning is about.
fn handle_paste_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, text: &str) -> Result<()> {
    if let Mode::Editing(editor) = &mut app.mode {
        editor.paste_text(text);
        return Ok(());
    }

    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        dispatch_key_event(app, crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), terminal)?;
    }
    Ok(())
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
            Event::Key(key) => {
                handle_key_event(app, key, terminal)?;
                return Ok(());
            }
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
///
/// Returns whether anything was actually dispatched -- `false` only for
/// a bare `key.kind != KeyEventKind::Press` (`dispatch_key_event`'s own
/// very first check no-ops on those unconditionally, for every mode).
/// Windows' Console API reports a real key-up `KeyEventKind::Release`
/// for every physical keypress, not just the down-stroke
/// (`is_repeatable_navigation_key`'s own doc comment already covers the
/// down-stroke's own repeat behavior) -- before this, `run()`'s own loop
/// redrew once for *that* too, a real, reported symptom: the terminal's
/// own real cursor briefly visits wherever `Release` leaves it during
/// that spurious redraw (a screen redraw always ends by repositioning
/// the terminal cursor for whatever `Mode` is now active) before the
/// state genuinely settles, reading as a visible flicker/jump on
/// certain terminals for keys whose own redraw is otherwise cheap
/// enough to land inside one visible refresh window (`Delete`/`Backspace`
/// on the command line, reported directly). Skipping the redraw
/// entirely for a key `dispatch_key_event` never touched removes that
/// spurious frame outright, for every key, not just the one reported.
fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<bool> {
    if key.kind == KeyEventKind::Press && should_swallow_paste_tail(app, key.code, key.modifiers) {
        return Ok(false);
    }

    dispatch_key_event(app, key, terminal)?;
    if key.kind == KeyEventKind::Press {
        if is_repeatable_navigation_key(key.code) {
            drain_pending_navigation_keys(app, terminal, key.code)?;
        } else if is_plain_typed_char(key.code, key.modifiers) && editor_accepting_plain_typing(app) {
            drain_pending_editor_typing(app, terminal)?;
        }
    }
    Ok(key.kind == KeyEventKind::Press)
}

/// See `App::pending_paste_swallow`'s own doc comment -- `true` means
/// "this key matches the next expected character of Windows Terminal's
/// own still-incoming paste flood, already applied instantly by
/// `try_intercept_paste_hotkey`; discard it rather than typing the same
/// text a second time." Pops the queue's front on every swallow, and
/// clears it entirely (without swallowing this particular key) once
/// either it drains naturally, `pending_paste_swallow_deadline` has
/// passed, or this key simply doesn't match what was expected next.
///
/// **Matches by content, not just by shape** -- reported directly:
/// pressing `Enter` several times right after a paste only registered
/// with a 5-10 second delay. An earlier version of this only checked
/// whether a key was *character-shaped* (a plain `Char` or bare
/// `Enter`, no `Ctrl`/`Alt`) before swallowing it, with a plain
/// decrementing counter -- which also matches perfectly ordinary
/// keystrokes the user types *during* the still-draining swallow
/// window (a real `Enter` looks identical in shape to a flood `Enter`),
/// so genuine typing got silently eaten and had to wait for the whole
/// window to finish before anything else could get through. Comparing
/// against the *actual* pasted text's own next character instead means
/// a real keystroke that doesn't happen to match what the flood would
/// send next (the overwhelming majority of the time) is recognized
/// immediately and handled right away, not swallowed.
///
/// The same real report from before this fix still applies to *why*
/// modifiers matter: `Ctrl+S`/`Ctrl+Z` must never match regardless of
/// their `Char` code, since Windows Terminal's own flood only ever
/// injects the pasted text's own literal, unmodified characters.
fn should_swallow_paste_tail(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if app.pending_paste_swallow.is_empty() {
        return false;
    }
    let expired = app.pending_paste_swallow_deadline.is_some_and(|deadline| std::time::Instant::now() > deadline);
    let typed = match code {
        KeyCode::Char(c) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
        KeyCode::Enter if modifiers.is_empty() => Some('\n'),
        _ => None,
    };
    let matches_next = typed.is_some_and(|c| app.pending_paste_swallow.front() == Some(&c));
    if expired || !matches_next {
        app.pending_paste_swallow.clear();
        app.pending_paste_swallow_deadline = None;
        return false;
    }
    app.pending_paste_swallow.pop_front();
    if app.pending_paste_swallow.is_empty() {
        app.pending_paste_swallow_deadline = None;
    }
    true
}

/// A `Char` key with no `Ctrl`/`Alt` held -- ordinary typed text
/// (`Shift` included, for an uppercase letter), never a shortcut.
/// `drain_pending_editor_typing`'s own gate for what's safe to fold
/// into one batched insert.
fn is_plain_typed_char(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::Char(_)) && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Whether the built-in editor is the thing that would actually receive
/// a plain typed character right now -- mirrors `dispatch_key_event`'s
/// own `Mode::Editing` guard exactly, including the embedded-Markdown-
/// preview carve-out (while the *preview* half has focus, plain
/// characters go to `explorer::handle_markdown_edit_preview_key`
/// instead, never the editor), plus `Editor::is_plain_standard_typing`'s
/// own keymap/mode check.
fn editor_accepting_plain_typing(app: &App) -> bool {
    let Mode::Editing(editor) = &app.mode else {
        return false;
    };
    if app.markdown_edit_preview.is_some() && app.active == 1 {
        return false;
    }
    editor.is_plain_standard_typing()
}

/// Reported directly, traced through the real symptom ("I can literally
/// watch it insert line by line"): pasting a real file (`ARCHITECTURE.md`,
/// 338 lines / 20KB) into the built-in editor took well over a minute
/// -- confirmed *not* to be `editor::fast_paste::splice_paste`'s own
/// cost (benchmarked directly against the real file: under 1ms) or
/// `EnableBracketedPaste` (`setup_terminal`'s own doc comment covers
/// that attempt) -- `crossterm` 0.29's own Windows backend
/// (`crossterm::event::sys::windows`) has no code path that ever
/// produces `Event::Paste` at all, confirmed directly from its source;
/// bracketed paste there is Unix-only, gated on parsing raw ANSI escape
/// bytes from stdin, which the Windows Console API backend (structured
/// `KEY_EVENT_RECORD`s via `ReadConsoleInputW`, not a raw byte stream)
/// never does. On Windows, an OS paste is genuinely indistinguishable
/// from very fast typing -- the terminal just injects the clipboard
/// text as a flood of ordinary simulated keystrokes, one at a time, and
/// `run()`'s own loop redraws once per key -- thousands of characters,
/// thousands of redraws, is exactly what "line by line" looks like from
/// the outside.
///
/// Mirrors `drain_pending_navigation_keys`'s own "coalesce a burst,
/// redraw once" shape: drains every already-queued plain character key
/// (`is_plain_typed_char`) for as long as the editor would still accept
/// one as plain typing (`editor_accepting_plain_typing` -- re-checked on
/// every iteration, not just once, in case some other queued event
/// changes mode mid-burst), and applies the whole batch in one
/// `Editor::paste_text` call -- the same fast, from-scratch splice
/// `Ctrl+V`/a real bracketed paste already use
/// (`editor::fast_paste_from_clipboard`'s own doc comment has the full
/// algorithmic story), rather than one `InsertChar` per character. A
/// key found mid-burst that isn't a plain character (or that arrives
/// once the editor would no longer treat one as plain typing) is never
/// silently dropped, same rule every sibling drain function in this
/// file already follows -- the batch collected so far is flushed first,
/// then that key/event is dispatched normally before this returns.
fn drain_pending_editor_typing(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut batch = String::new();
    while event::poll(std::time::Duration::from_secs(0))? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press && is_plain_typed_char(key.code, key.modifiers) && editor_accepting_plain_typing(app) => {
                if let KeyCode::Char(c) = key.code {
                    batch.push(c);
                }
            }
            Event::Key(key) => {
                flush_editor_typing_batch(app, &mut batch);
                return dispatch_key_event(app, key, terminal);
            }
            Event::Mouse(mouse) => {
                flush_editor_typing_batch(app, &mut batch);
                explorer::handle_markdown_preview_mouse(app, mouse);
                let last_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown).then_some(mouse.kind);
                return drain_pending_mouse_events(app, terminal, last_scroll);
            }
            _ => {}
        }
    }
    flush_editor_typing_batch(app, &mut batch);
    Ok(())
}

/// Applies `batch` (if non-empty) to the editor via `Editor::paste_text`
/// and clears it -- shared by every early-return branch in
/// `drain_pending_editor_typing` above, so a batch collected so far is
/// never silently lost just because the drain is about to end for a
/// different reason.
fn flush_editor_typing_batch(app: &mut App, batch: &mut String) {
    if batch.is_empty() {
        return;
    }
    if let Mode::Editing(editor) = &mut app.mode {
        editor.paste_text(batch);
    }
    batch.clear();
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

    // See `keyboard_layout::normalize_ctrl_shortcut`'s own doc comment
    // -- a `Ctrl+<letter>` chord typed under a non-Latin layout (a real
    // report: `Ctrl+C` in the editor did nothing at all under a Russian
    // layout) arrives with a layout-translated `char`, not the Latin
    // one every binding in this app is written against. Normalized once
    // here, ahead of every mode's own key handling, rather than in each
    // of them separately.
    let key = keyboard_layout::normalize_ctrl_shortcut(key);

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
        Mode::CompareFiles(_) => compare::handle_compare_key(app, key),
        Mode::CompareMenu(_, _) => compare::handle_compare_menu_key(app, key),
        Mode::CompareLineEndingMenu(_, _) => compare::handle_compare_line_ending_menu_key(app, key),
        Mode::CompareConfirmDiscard(_) => compare::handle_compare_confirm_discard_key(app, key),
        Mode::ConfirmDelete(_) => explorer::handle_confirm_delete_key(app, key),
        Mode::ConfirmTransfer(_) => explorer::handle_confirm_transfer_key(app, key),
        Mode::MainMenu(_) => theming::handle_main_menu_key(app, key),
        Mode::ThemeMenu(_) => theming::handle_theme_menu_key(app, key),
        Mode::ShellMenu(_) => command_line::handle_shell_menu_key(app, key),
        Mode::PopupStyleMenu(_) => theming::handle_popup_style_menu_key(app, key),
        Mode::FindFile(_) => explorer::handle_find_file_key(app, key),
        Mode::CommandHistory(_) => command_line::handle_history_key(app, key, terminal),
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    mod should_swallow_paste_tail_tests {
        use super::*;

        fn armed(chars: &str) -> (App, std::time::Instant) {
            let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
            app.pending_paste_swallow = chars.chars().collect();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            app.pending_paste_swallow_deadline = Some(deadline);
            (app, deadline)
        }

        #[test]
        fn nothing_pending_never_swallows() {
            let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
            assert!(!should_swallow_paste_tail(&mut app, KeyCode::Char('x'), KeyModifiers::NONE));
        }

        #[test]
        fn swallows_matching_chars_and_enter_in_order_until_the_queue_drains() {
            let (mut app, _) = armed("a\n");

            assert!(should_swallow_paste_tail(&mut app, KeyCode::Char('a'), KeyModifiers::NONE));
            assert_eq!(app.pending_paste_swallow, ['\n']);
            assert!(should_swallow_paste_tail(&mut app, KeyCode::Enter, KeyModifiers::NONE));
            assert!(app.pending_paste_swallow.is_empty());
            assert!(app.pending_paste_swallow_deadline.is_none(), "should clear its own deadline once the queue naturally drains");

            assert!(!should_swallow_paste_tail(&mut app, KeyCode::Char('z'), KeyModifiers::NONE), "a real keystroke after the queue is drained must not be swallowed");
        }

        #[test]
        fn a_key_that_is_not_a_char_or_enter_ends_the_swallow_without_eating_it() {
            let (mut app, _) = armed("hello");

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Left, KeyModifiers::NONE);

            assert!(!swallowed, "an unrelated key must never be silently discarded");
            assert!(app.pending_paste_swallow.is_empty(), "the mismatch should end the whole swallow window, not just skip this one key");
        }

        /// Real reported bug: `Ctrl+S` (save) and `Ctrl+Z` (undo the very
        /// paste this swallow exists for) right after a large paste were
        /// silently eaten instead of running -- both are `KeyCode::Char`
        /// too, and the swallow used to key off `code` alone, ignoring
        /// `modifiers` entirely. Windows Terminal's own flood only ever
        /// injects *plain* characters (no `Ctrl`/`Alt`), so a `Ctrl`-held
        /// `Char` must never be treated as part of it, even if its own
        /// letter happens to match the next expected flood character.
        #[test]
        fn a_ctrl_held_char_is_never_swallowed_even_mid_flood() {
            let (mut app, _) = armed("stuff");

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

            assert!(!swallowed, "Ctrl+S must reach the editor, not be eaten as flood tail");
            assert!(app.pending_paste_swallow.is_empty(), "a real shortcut mid-flood should end the swallow window entirely");
        }

        /// Real reported bug: pressing `Enter` several times right after a
        /// paste only registered several seconds late. Root cause: the
        /// swallow used to match on *shape* alone (any plain `Char`/bare
        /// `Enter`), so a real `Enter` typed while the flood was still
        /// mid-drain looked identical to one of the flood's own and got
        /// eaten too. Content-matching against the actual next expected
        /// character fixes this: a real `Enter` that doesn't match
        /// whatever the flood would send next must be handled immediately,
        /// not swallowed and delayed.
        #[test]
        fn a_real_keystroke_that_does_not_match_the_next_expected_character_is_handled_immediately() {
            let (mut app, _) = armed("hello world"); // next expected char is 'h', not Enter

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Enter, KeyModifiers::NONE);

            assert!(!swallowed, "a real Enter that doesn't match the flood's own next character must not be delayed");
            assert!(app.pending_paste_swallow.is_empty(), "the mismatch ends the swallow window entirely, so nothing further gets delayed either");
        }

        #[test]
        fn an_expired_deadline_ends_the_swallow_even_for_a_matching_key() {
            let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
            app.pending_paste_swallow = "a".chars().collect();
            app.pending_paste_swallow_deadline = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

            assert!(!swallowed, "past the safety-valve deadline, real typing should never be eaten even if it happens to match");
            assert!(app.pending_paste_swallow.is_empty());
        }
    }
}
