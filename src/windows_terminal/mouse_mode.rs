//! Whether the console still gives litastum the mouse. `EnableMouseCapture`
//! sets the console's input mode once (mouse input on, QuickEdit off); a
//! process sharing the console can set it back -- then Windows Terminal
//! selected with the mouse itself, line numbers and all, in its own color
//! (reported, in the editor, without `Shift`). Checked every loop
//! iteration and set again (`event_loop::sync::sync_mouse_capture`).

use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, ENABLE_MOUSE_INPUT, ENABLE_QUICK_EDIT_MODE, STD_INPUT_HANDLE};


/// The console's input mode now; `None` when there's no console to ask
/// (redirected input).
pub fn input_mode() -> Option<u32> {
    let mut mode = 0;
    // SAFETY: `GetStdHandle` returns a handle owned by the process (never
    // closed here), and `GetConsoleMode` only writes `mode`.
    let ok = unsafe { GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &mut mode) };
    (ok != 0).then_some(mode)
}


/// Whether a console in `mode` sends the mouse to the program rather than
/// selecting with it.
pub fn takes_the_mouse(mode: u32) -> bool {
    mode & ENABLE_MOUSE_INPUT != 0 && mode & ENABLE_QUICK_EDIT_MODE == 0
}


#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Console::{ENABLE_EXTENDED_FLAGS, ENABLE_WINDOW_INPUT};

    use super::*;

    #[test]
    fn the_mouse_is_ours_with_mouse_input_on_and_quick_edit_off() {
        assert!(takes_the_mouse(ENABLE_MOUSE_INPUT | ENABLE_WINDOW_INPUT | ENABLE_EXTENDED_FLAGS), "as EnableMouseCapture sets it");
        assert!(!takes_the_mouse(ENABLE_WINDOW_INPUT | ENABLE_EXTENDED_FLAGS), "mouse input off");
        assert!(!takes_the_mouse(ENABLE_MOUSE_INPUT | ENABLE_QUICK_EDIT_MODE | ENABLE_EXTENDED_FLAGS), "QuickEdit selects");
        assert!(!takes_the_mouse(0));
    }
}
