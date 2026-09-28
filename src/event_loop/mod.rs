use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, MouseEventKind};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::App;
use crate::{explorer, ui};

mod keys;
mod paste;

use keys::handle_key_event;
use paste::handle_paste_event;
#[cfg(windows)]
use paste::try_intercept_paste_hotkey;

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

pub(crate) fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
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
/// physical `Alt` key's actual held state (`windows_terminal::alt_key::is_physically_down`)
/// changes, so the alt-labels F-key row can react to `Alt` genuinely
/// being held down, not just to the next keypress that happens to carry
/// the `Alt` modifier. See `windows_terminal/alt_key.rs`'s own doc for why that
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
        // until the flood was already over. See `windows_terminal`'s own
        // module doc comment for the full story.
        if try_intercept_paste_hotkey(app, terminal)? {
            return Ok(());
        }
        let poll_interval = if background_task_pending(app) { BACKGROUND_TASK_POLL_INTERVAL } else { crate::windows_terminal::alt_key::POLL_INTERVAL };
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
        let alt_down = crate::windows_terminal::alt_key::is_physically_down();
        if alt_down != app.alt_held {
            app.alt_held = alt_down;
            return Ok(());
        }
    }
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
