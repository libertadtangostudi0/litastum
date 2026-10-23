use color_eyre::eyre::Result;
use crossterm::event::{self, Event, MouseEvent, MouseEventKind};

use crate::app::{App, Mode};
use crate::{explorer, ui};
use crate::terminal_setup::Tui;

mod keys;
mod paste;
mod sync;
#[cfg(windows)]
mod zoom;

use keys::handle_key_event;
use paste::handle_paste_event;
#[cfg(windows)]
use paste::try_intercept_paste_hotkey;
use sync::{sync_mouse_capture, sync_panel_paths, sync_screen, sync_terminal_palette, sync_title};
#[cfg(windows)]
pub(crate) use zoom::save_terminal_zoom;
#[cfg(windows)]
use zoom::{settle_terminal_zoom, sync_terminal_zoom};

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

/// Whether an image decode, a Find file search or a file job is in flight, or the
/// panels' directories wait to be written (`last_paths`).
fn background_task_pending(app: &App) -> bool {
    explorer::is_image_decode_pending(app) || explorer::is_find_file_search_pending(app) || explorer::is_file_job_pending(app) || app.last_paths.pending() || app.clipboard_wipe.pending()
}

/// Polls every background task once; `true` if one finished and the
/// screen should redraw. Writing the panels' directories, once due,
/// changes nothing on screen.
fn poll_background_tasks(app: &mut App) -> bool {
    if let Some(paths) = app.last_paths.take_due(std::time::Instant::now()) {
        crate::last_paths::save(&paths);
    }
    app.clipboard_wipe.poll(std::time::Instant::now());
    let image = explorer::poll_pending_image_decode(app);
    let search = explorer::poll_pending_find_file_search(app);
    let job = explorer::poll_pending_file_job(app);
    image || search || job
}

/// How often `wait_for_event` looks at the physical `Ctrl+V` while idle
/// on Windows (`try_intercept_paste_hotkey`).
#[cfg(windows)]
const IDLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

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
    // `Shift+F2`'s field over the title takes its own clicks and drags.
    if matches!(app.mode, Mode::Editing(_)) && app.editor_save_as.as_mut().is_some_and(|edit| edit.mouse(mouse)) {
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
