//! Windows Terminal's zoom per screen, from the event loop's side
//! (`crate::terminal_zoom`): the user's presses counted, the screen's
//! steps pressed, the resizes that follow waited out, a change saved.

use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{self, Event};

use crate::app::App;
use crate::terminal_setup::Tui;
use crate::windows_terminal::zoom_keys;

/// How long Windows Terminal may take to start changing size after
/// litastum's zoom presses, and between its steps.
const ZOOM_FIRST_RESIZE: Duration = Duration::from_millis(150);
const ZOOM_NEXT_RESIZE: Duration = Duration::from_millis(60);
/// The longest litastum waits for the terminal's zoom to settle.
const ZOOM_SETTLE_MAX: Duration = Duration::from_millis(800);


/// The user's presses counted for the screen shown, the screen's steps
/// pressed once no key is held and the terminal is in front, a change
/// saved. `true` when it pressed keys: the terminal is changing size.
pub(super) fn sync_terminal_zoom(app: &mut App) -> bool {
    let screen = super::sync::screen_name(&app.mode);
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


/// Waits out Windows Terminal's zoom after litastum's presses -- it
/// changes size a step at a time, and drawing at each step showed the
/// zoom crawl (reported): the resizes are let by until they stop, so the
/// next frame is drawn once, at the final size. Any other event ends the
/// wait and is handled.
pub(super) fn settle_terminal_zoom(app: &mut App, terminal: &mut Tui) -> Result<()> {
    let deadline = Instant::now() + ZOOM_SETTLE_MAX;
    let mut quiet = ZOOM_FIRST_RESIZE;
    loop {
        let wait = quiet.min(deadline.saturating_duration_since(Instant::now()));
        if wait.is_zero() || !event::poll(wait)? {
            return Ok(());
        }
        match event::read()? {
            Event::Resize(..) => quiet = ZOOM_NEXT_RESIZE,
            other => {
                super::dispatch_event(app, terminal, other)?;
                return Ok(());
            }
        }
    }
}


/// Saves a changed zoom per screen and where the tab is (`terminal_zoom`).
pub(crate) fn save_terminal_zoom(zoom: &mut crate::terminal_zoom::TerminalZoom) {
    if let Some((steps, now)) = zoom.take_unsaved() {
        let now = crate::theming::config::TerminalZoomNow { session: zoom_keys::session(), steps: now };
        crate::theming::config::save_terminal_zoom(&steps, now);
    }
}
