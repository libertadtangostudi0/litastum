//! Polls the real, physical state of `Ctrl+V` directly via the Windows
//! API, bypassing `crossterm`/the terminal entirely for this one
//! signal — the same technique `alt_key.rs` already uses for `Alt`,
//! for a related but distinct reason.
//!
//! **The problem this exists to fix**: pasting a large file's worth of
//! text into the built-in editor was reported as taking well over a
//! minute, traced (via `logs/litastum.log`'s own timestamps on
//! consecutive `editor key` entries) to a rock-steady ~7-8ms gap
//! between *every single character* of the paste. That rules out this
//! app's own redraw/insert cost (already confirmed separately, both
//! under 1ms for a real 20KB file) -- what's actually happening is that
//! Windows Terminal intercepts `Ctrl+V` as *its own* "paste" key
//! binding, reads the OS clipboard itself, and injects the text into
//! the console's input stream as a burst of ordinary simulated
//! keystrokes, at that same throttled rate, because `crossterm`'s
//! Windows backend (confirmed directly from its own source -- no
//! `Event::Paste`-related code exists there at all) has no bracketed-
//! paste support to ask for anything better. Two consequences neither
//! of which this app's own code can fix from inside the normal
//! `crossterm` event stream:
//!
//! 1. This app never receives a real `Ctrl+V` `KeyEvent` for a paste at
//!    all -- Windows Terminal consumes it before forwarding anything,
//!    so `editor::fast_paste_from_clipboard`'s own interception
//!    (`Editor::input`) never fires for a real user paste, only for a
//!    synthetic one built in a test.
//! 2. Even `event_loop::keys::drain_pending_editor_typing`'s own "batch whatever
//!    the queue already has" fix (built for the *previous* theory --
//!    kept for genuine fast typing/other bursts, and harmless here)
//!    can't help either: characters simply aren't queued up in advance
//!    to batch. They trickle in one at a time, ~7-8ms apart, so by the
//!    time this app polls for more, there usually isn't anything else
//!    there yet.
//!
//! The only way around both is to stop waiting for `crossterm` to ever
//! report a `Ctrl+V` `KeyEvent` and instead catch the real, physical
//! key combo directly from the OS -- `GetAsyncKeyState`, exactly
//! `alt_key.rs`'s own approach -- read the clipboard ourselves the
//! instant it's detected, and apply the same fast splice `Ctrl+V`
//! itself would have (`Editor::paste_text`). `event_loop::wait_for_event`
//! checks this on every loop iteration (not just once per call, the
//! way `alt_key`'s own idle-only check works) -- during an active
//! keystroke flood this app is still processing individual characters
//! constantly, so an idle-only check would never run until the flood
//! is already over. Windows Terminal's own injected flood keeps
//! arriving right afterward regardless (there's no way to tell it to
//! stop) -- `event_loop::paste::try_intercept_paste_hotkey`'s own doc comment
//! covers how that tail gets silently discarded instead of typed a
//! second time.

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
