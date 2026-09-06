//! Polls the real, physical state of the `Alt` key directly via the
//! Windows API, bypassing `crossterm` entirely for this one signal.
//!
//! Needed because `crossterm`'s own Windows Console backend never
//! emits a `KeyEvent` at all for a bare `Shift`/`Ctrl`/`Alt` press or
//! release on its own — only the modifier *flags* riding along with an
//! actual keypress (confirmed in `crossterm`'s own source:
//! `event/sys/windows/parse.rs` matches `VK_SHIFT | VK_CONTROL |
//! VK_MENU => None`, and `supports_keyboard_enhancement()` — the Kitty
//! protocol escape hatch that reports standalone modifier events on
//! terminals that support it — is hardcoded to `Ok(false)` on Windows).
//! That made real "Alt held down" tracking impossible from `crossterm`'s
//! event stream alone: `App::alt_held` only ever updated in the same
//! frame an `Alt+`-something shortcut already fired, which defeats the
//! whole point of a preview row (by the time it shows, the shortcut
//! already ran). `GetAsyncKeyState` reports the true current key state
//! regardless of whether a terminal input event was ever generated for
//! it, so `main.rs::wait_for_event` polls this while otherwise idle.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};

/// How often `main.rs::wait_for_event` checks this while idle, waiting
/// for a real terminal event. Small enough that the alt-labels row
/// feels instant when Alt is pressed or released; large enough not to
/// burn CPU polling a key that changes state rarely.
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// Whether the physical `Alt` key (either left or right — `VK_MENU`
/// covers both) is currently held down, straight from the OS rather
/// than from any terminal input event.
pub fn is_physically_down() -> bool {
    // The high bit set means "currently down" -- see GetAsyncKeyState's
    // own docs. The low bit (whether it was pressed since the last
    // call) is irrelevant here; only live-held state matters.
    unsafe { (GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000) != 0 }
}
