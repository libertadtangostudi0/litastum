use color_eyre::eyre::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseEvent, MouseEventKind};
use crossterm::execute;
use tracing::warn;

use crate::app::{App, Mode};
use crate::{explorer, ui};
use crate::terminal_setup::Tui;

mod keys;
mod paste;

use keys::handle_key_event;
use paste::handle_paste_event;
#[cfg(windows)]
use paste::try_intercept_paste_hotkey;

/// Draws one frame, then places the terminal cursor `ui::draw` returned
/// -- position first, then `show_cursor`, after the frame is on screen.
/// Letting `ratatui` do it shows the cursor before moving it, which
/// flickered. History: docs/history/event-loop.md.
fn draw_and_apply_cursor(terminal: &mut Tui, app: &mut App) -> Result<[(usize, usize); 2]> {
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

pub(crate) fn run(terminal: &mut Tui, app: &mut App) -> Result<()> {
    // One draw up front seeds the panels' real columns/visible_rows;
    // otherwise the first visible frame uses `Panel::new()`'s one-column
    // placeholder and keeps it until the next key.
    let layout = draw_and_apply_cursor(terminal, app)?;
    for (panel, (cols, rows)) in app.panels.iter_mut().zip(layout) {
        panel.set_columns(cols);
        panel.set_visible_rows(rows);
    }

    while !app.should_quit {
        sync_mouse_capture(app);
        sync_terminal_palette(app);
        sync_title(app);
        sync_screen(app);
        #[cfg(windows)]
        if sync_terminal_zoom(app) {
            settle_terminal_zoom(app, terminal)?;
        }
        sync_panel_paths(app);
        let layout = draw_and_apply_cursor(terminal, app)?;
        for (panel, (cols, rows)) in app.panels.iter_mut().zip(layout) {
            panel.set_columns(cols);
            panel.set_visible_rows(rows);
        }
        wait_for_event(app, terminal)?;
    }
    Ok(())
}


/// How often `wait_for_event` polls a background image decode or Find
/// file search while one runs, so a finished result shows at once
/// without waiting for input.
const BACKGROUND_TASK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(30);

/// Whether an image decode or a Find file search is in flight, or the
/// panels' directories wait to be written (`last_paths`).
fn background_task_pending(app: &App) -> bool {
    explorer::is_image_decode_pending(app) || explorer::is_find_file_search_pending(app) || app.last_paths.pending()
}

/// Polls every background task once; `true` if one finished and the
/// screen should redraw. Writing the panels' directories, once due,
/// changes nothing on screen.
fn poll_background_tasks(app: &mut App) -> bool {
    if let Some(paths) = app.last_paths.take_due(std::time::Instant::now()) {
        crate::last_paths::save(&paths);
    }
    let image = explorer::poll_pending_image_decode(app);
    let search = explorer::poll_pending_find_file_search(app);
    image || search
}

/// How often `wait_for_event` looks at the physical `Ctrl+V` while idle
/// on Windows (`try_intercept_paste_hotkey`).
#[cfg(windows)]
const IDLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// Windows Terminal's zoom per screen (`terminal_zoom`): the user's
/// presses counted for the screen shown, the screen's steps pressed once
/// no key is held and the terminal is in front, a change saved. `true`
/// when it pressed keys: the terminal is changing size.
#[cfg(windows)]
fn sync_terminal_zoom(app: &mut App) -> bool {
    use crate::windows_terminal::zoom_keys;

    let screen = screen_name(&app.mode);
    let focused = app.terminal_focused;
    let Some(zoom) = &mut app.terminal_zoom else {
        return false;
    };
    zoom_keys::set_focused(focused);
    // Presses made on the screen before this one was shown count for it:
    // taken before the switch.
    let (steps, reset) = zoom_keys::take_presses();
    if reset {
        zoom.reset();
    }
    zoom.stepped(steps);
    zoom.set_screen(screen);
    let presses = zoom.presses_needed();
    let press = presses != 0 && focused && !zoom_keys::keys_held() && zoom_keys::terminal_in_front();
    if press {
        zoom_keys::press(presses);
        zoom.pressed(presses);
    }
    save_terminal_zoom(zoom);
    press
}


/// How long Windows Terminal may take to start changing size after
/// litastum's zoom presses, and between its steps.
#[cfg(windows)]
const ZOOM_FIRST_RESIZE: std::time::Duration = std::time::Duration::from_millis(150);
#[cfg(windows)]
const ZOOM_NEXT_RESIZE: std::time::Duration = std::time::Duration::from_millis(60);
/// The longest litastum waits for the terminal's zoom to settle.
#[cfg(windows)]
const ZOOM_SETTLE_MAX: std::time::Duration = std::time::Duration::from_millis(800);


/// Waits out Windows Terminal's zoom after litastum's presses -- it
/// changes size a step at a time, and drawing at each step showed the
/// zoom crawl (reported): the resizes are let by until they stop, so the
/// next frame is drawn once, at the final size. Any other event ends the
/// wait and is handled.
#[cfg(windows)]
fn settle_terminal_zoom(app: &mut App, terminal: &mut Tui) -> Result<()> {
    let deadline = std::time::Instant::now() + ZOOM_SETTLE_MAX;
    let mut quiet = ZOOM_FIRST_RESIZE;
    loop {
        let wait = quiet.min(deadline.saturating_duration_since(std::time::Instant::now()));
        if wait.is_zero() || !event::poll(wait)? {
            return Ok(());
        }
        match event::read()? {
            Event::Resize(..) => quiet = ZOOM_NEXT_RESIZE,
            other => {
                dispatch_event(app, terminal, other)?;
                return Ok(());
            }
        }
    }
}


/// Saves a changed zoom per screen and where the tab is (`terminal_zoom`).
#[cfg(windows)]
pub(crate) fn save_terminal_zoom(zoom: &mut crate::terminal_zoom::TerminalZoom) {
    if let Some((steps, now)) = zoom.take_unsaved() {
        let now = crate::theming::config::TerminalZoomNow { session: crate::windows_terminal::zoom_keys::session(), steps: now };
        crate::theming::config::save_terminal_zoom(&steps, now);
    }
}


/// Blocks until a terminal event arrives (`handle_event`), a background
/// task finishes, or -- on Windows -- `Ctrl+V` is pressed (Windows
/// Terminal never sends it, so it's polled).
#[cfg(windows)]
fn wait_for_event(app: &mut App, terminal: &mut Tui) -> Result<()> {
    loop {
        // Every iteration, not only when idle: during Windows Terminal's
        // paste flood there's always an event waiting.
        if try_intercept_paste_hotkey(app)? {
            return Ok(());
        }
        if sync_terminal_zoom(app) {
            settle_terminal_zoom(app, terminal)?;
            return Ok(());
        }
        let poll_interval = if background_task_pending(app) { BACKGROUND_TASK_POLL_INTERVAL } else { IDLE_POLL_INTERVAL };
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
    }
}

#[cfg(not(windows))]
fn wait_for_event(app: &mut App, terminal: &mut Tui) -> Result<()> {
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


/// Returns whether the caller should redraw -- `false` for a key nothing
/// was dispatched for.
fn handle_event(app: &mut App, terminal: &mut Tui) -> Result<bool> {
    let event = event::read()?;
    dispatch_event(app, terminal, event)
}


/// `handle_event` for an event already read.
fn dispatch_event(app: &mut App, terminal: &mut Tui, event: Event) -> Result<bool> {
    if app.note_focus(&event) {
        return Ok(false);
    }
    match event {
        Event::Key(key) => handle_key_event(app, key, terminal),
        // Only arrives in the modes `sync_mouse_capture` captures for.
        Event::Mouse(mouse) => {
            handle_mouse(app, mouse);
            let last_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown).then_some(mouse.kind);
            drain_pending_mouse_events(app, terminal, last_scroll)?;
            Ok(true)
        }
        Event::Paste(text) => {
            handle_paste_event(app, &text)?;
            Ok(true)
        }
        _ => Ok(true),
    }
}

/// Handles every queued mouse event without blocking, so a touchpad's
/// burst of wheel ticks costs one redraw, not one per tick. Drains until
/// the queue is empty (a time cap made long scrolls feel slower), but
/// stops right after a scroll that reverses direction (`last_scroll`) --
/// that's the next gesture, not more of this one. A key found mid-burst
/// is handled and ends the drain. History: docs/history/event-loop.md.
fn drain_pending_mouse_events(app: &mut App, terminal: &mut Tui, mut last_scroll: Option<MouseEventKind>) -> Result<()> {
    while event::poll(std::time::Duration::from_secs(0))? {
        match event::read()? {
            Event::Mouse(mouse) => {
                let is_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
                let reversed = is_scroll && last_scroll.is_some_and(|prev| prev != mouse.kind);
                handle_mouse(app, mouse);
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
            other => {
                app.note_focus(&other);
            }
        }
    }
    Ok(())
}


/// Mouse capture is on in the browser (a click on a panel's title edits
/// its path) and while an editor is open (F4, Compare or the conflict
/// resolver), synced once per loop iteration rather than at each place one
/// opens or closes. Capture takes over the terminal's own text selection
/// (Windows Terminal still selects with `Shift` held); the user screen has
/// its own (`user_screen/selection.rs`). Only set after the terminal call
/// succeeds -- `restore_terminal` relies on it.
fn sync_mouse_capture(app: &mut App) {
    let wanted = matches!(app.mode, Mode::Browsing | Mode::Editing(_) | Mode::CompareFiles(_) | Mode::ResolveConflict(_));
    if wanted == app.mouse_capture_enabled {
        return;
    }
    let result = if wanted { execute!(std::io::stdout(), EnableMouseCapture) } else { execute!(std::io::stdout(), DisableMouseCapture) };
    match result {
        Ok(()) => app.mouse_capture_enabled = wanted,
        Err(err) => warn!(%err, wanted, "failed to toggle mouse capture"),
    }
}


/// The terminal's title (`OSC 2`): the active panel's directory, or the
/// file being edited -- what a tab is labelled with, in litastum's own
/// window and in Windows Terminal alike. Sent only when it changes.
fn sync_title(app: &mut App) {
    let wanted = window_title(app);
    if app.terminal_title.as_deref() == Some(wanted.as_str()) {
        return;
    }
    if let Err(err) = execute!(std::io::stdout(), crossterm::terminal::SetTitle(&wanted)) {
        warn!(%err, "failed to set the terminal's title");
        return;
    }
    app.terminal_title = Some(wanted);
}


/// Notes the panels' directories for the next start (`last_paths`): a
/// change is written 30 s later, from the background-task polling below.
fn sync_panel_paths(app: &mut App) {
    let current = [app.panels[0].path.clone(), app.panels[1].path.clone()];
    app.last_paths.note(current, std::time::Instant::now());
}


/// Tells the terminal which screen litastum is on (`screen_name`) as the
/// terminal user variable `litastum_screen` (`OSC 1337 ; SetUserVar`,
/// WezTerm's convention; other terminals ignore it). litastum's window
/// keeps a zoom per screen with it. Sent only when it changes.
fn sync_screen(app: &mut App) {
    let wanted = screen_name(&app.mode);
    if app.terminal_screen == Some(wanted) {
        return;
    }
    use std::io::Write;
    let mut stdout = std::io::stdout();
    if let Err(err) = stdout.write_all(screen_sequence(wanted).as_bytes()).and_then(|()| stdout.flush()) {
        warn!(%err, "failed to tell the terminal the screen");
        return;
    }
    app.terminal_screen = Some(wanted);
}


/// The screen's name for its zoom (requested: the panels, the editor,
/// Compare and the resolver each keep their own). `main` is the panels --
/// an image preview included -- the name it had before the editor got
/// its own, so a saved zoom stays.
fn screen_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Editing(_) => "editor",
        Mode::CompareFiles(_) => "compare",
        Mode::ResolveConflict(_) => "conflict",
        Mode::Browsing | Mode::ImagePreview(_) => "main",
    }
}


/// `OSC 1337 ; SetUserVar=litastum_screen=<base64 of screen>`.
fn screen_sequence(screen: &str) -> String {
    format!("\x1b]1337;SetUserVar=litastum_screen={}\x07", base64(screen.as_bytes()))
}


/// Standard base64, which `SetUserVar` wants its value in.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}


/// The name a tab shows: the edited file's or the directory's, the whole
/// path for a drive's root.
fn window_title(app: &App) -> String {
    let path = match &app.mode {
        Mode::Editing(editor) => editor.path().to_path_buf(),
        Mode::CompareFiles(_) => return "Compare".to_string(),
        Mode::ResolveConflict(_) => return "Conflict".to_string(),
        _ => app.panels[app.active].path.clone(),
    };
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}


/// Hands the theme's colors to the terminal whenever they change -- on
/// the first frame and after F9 -> Color schemes -- rather than at each
/// place the theme is set. `restore_terminal` gives the terminal its own
/// back.
fn sync_terminal_palette(app: &mut App) {
    let wanted = crate::terminal_palette::palette_sequence(&app.theme);
    if wanted == app.terminal_palette {
        return;
    }
    if let Some(sequence) = &wanted {
        use std::io::Write;
        let mut stdout = std::io::stdout();
        if let Err(err) = stdout.write_all(sequence.as_bytes()).and_then(|()| stdout.flush()) {
            warn!(%err, "failed to set the terminal's colors");
            return;
        }
    }
    app.terminal_palette = wanted;
}


/// One mouse event: the editor gets it when it lands on the editor
/// (`Editor::contains_screen_position`), a linked Markdown preview gets
/// everything else (`explorer::handle_markdown_preview_mouse`, which
/// no-ops without one) -- so the wheel scrolls whichever of the two is
/// under the pointer, not both. A click into the editor half of an
/// editor+preview session also gives it keyboard focus (`App::active`).
/// Compare and the resolver route their own (`CompareState::mouse`,
/// `ConflictState::mouse`).
fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    let on_editor = matches!(app.mode, Mode::Editing(_) | Mode::CompareFiles(_) | Mode::ResolveConflict(_));
    let mouse = crate::cell_halves::adjust(&mut app.cell_halves, mouse, on_editor);
    // An overlay is modal: nothing underneath reacts to the mouse.
    if app.overlay.is_some() {
        return;
    }
    if let Mode::CompareFiles(state) = &mut app.mode {
        state.mouse(mouse);
        return;
    }
    if let Mode::ResolveConflict(state) = &mut app.mode {
        state.mouse(mouse);
        return;
    }
    if matches!(app.mode, Mode::Browsing) {
        crate::command_line::handle_browsing_mouse(app, mouse);
        return;
    }
    if let Mode::Editing(editor) = &mut app.mode {
        if editor.contains_screen_position(mouse.column, mouse.row) {
            editor.mouse(mouse);
            if app.markdown_edit_preview.is_some() && matches!(mouse.kind, MouseEventKind::Down(_)) {
                app.active = 0;
            }
            return;
        }
    }
    explorer::handle_markdown_preview_mouse(app, mouse);
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    /// Requested: the panels, the editor, Compare and the resolver each
    /// zoom on their own.
    #[test]
    fn the_editor_is_a_screen_of_its_own() {
        let dir = crate::test_support::unique_scratch_dir("screen-name");
        let file = dir.join("file.txt");
        std::fs::write(&file, "text").unwrap();
        let editor = crate::editor::Editor::open(file, None, crate::editor::EditorKeymapMode::Standard).unwrap();

        assert_eq!(screen_name(&Mode::Browsing), "main");
        assert_eq!(screen_name(&Mode::Editing(editor)), "editor");
    }

    #[test]
    fn the_screen_goes_out_as_a_base64_user_variable() {
        assert_eq!(base64(b"main"), "bWFpbg==");
        assert_eq!(base64(b"compare"), "Y29tcGFyZQ==");
        assert_eq!(base64(b"conflict"), "Y29uZmxpY3Q=");
        assert_eq!(screen_sequence("compare"), "\x1b]1337;SetUserVar=litastum_screen=Y29tcGFyZQ==\x07");
    }

    /// A tab is labelled with the active panel's directory.
    #[test]
    fn the_title_is_the_active_panels_directory() {
        let dir = unique_scratch_dir("title");
        let app = test_app(dir.clone());

        assert_eq!(window_title(&app), dir.file_name().unwrap().to_string_lossy());
    }
}
