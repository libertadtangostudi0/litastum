//! Polls the physical `Alt` key via the Windows API. `crossterm`'s Windows
//! backend emits no event for a bare modifier (`VK_SHIFT | VK_CONTROL |
//! VK_MENU => None`, and no Kitty keyboard protocol there), so
//! `App::alt_held` only changed together with an `Alt+` shortcut -- too
//! late for the preview row. `event_loop::wait_for_event` polls this
//! while idle.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};

/// How often `event_loop::wait_for_event` checks this while idle, waiting
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
