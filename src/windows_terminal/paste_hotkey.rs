//! Polls the real, physical state of `Ctrl+V` via the Windows API.
//!
//! Windows Terminal owns `Ctrl+V`: the app never receives the key event,
//! only the clipboard fed in afterward as simulated keystrokes at
//! ~7-8ms each -- a large paste took minutes. `event_loop::wait_for_event`
//! checks this on every loop iteration (not only when idle, like
//! `alt_key` -- during a flood the loop is never idle), and
//! `event_loop::paste::try_intercept_paste_hotkey` pastes the clipboard
//! at once; `PasteFlood` swallows the flood that follows.
//!
//! History: docs/history/editor-performance.md.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL};

/// `'V'` has no named `VK_*` constant in `windows-sys`' own bindings
/// (unlike `VK_MENU`/`VK_CONTROL`) -- virtual-key codes for the
/// alphanumeric row are just their own ASCII values, `GetAsyncKeyState`'s
/// own documented convention.
const VK_V: i32 = b'V' as i32;

/// Whether `Ctrl` and `V` are *both* physically held down right now,
/// straight from the OS rather than from any terminal input event --
/// see the module's own doc comment for why this is the only reliable
/// way to catch a real `Ctrl+V` paste at all on Windows.
pub fn ctrl_v_physically_down() -> bool {
    // The high bit set means "currently down" -- see GetAsyncKeyState's
    // own docs, same convention `alt_key::is_physically_down` already
    // relies on.
    let ctrl = unsafe { (GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0 };
    let v = unsafe { (GetAsyncKeyState(VK_V) as u16 & 0x8000) != 0 };
    ctrl && v
}
