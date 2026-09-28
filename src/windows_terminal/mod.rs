//! Workarounds for how Windows Terminal -- and the Windows console input
//! API underneath it, which is what `crossterm`'s Windows backend reads
//! -- deliver input differently from what `crossterm` (and the rest of
//! this app) otherwise assume. Collected in one place on request, so the
//! "why does this code poll the OS directly / swallow keystrokes" parts
//! don't sit scattered across the event loop and `App`:
//!
//! - `alt_key` -- a bare `Alt` press/release never produces an event at
//!   all, so the alternate F-key row polls the physical key state
//!   (`GetAsyncKeyState`) instead.
//! - `paste_hotkey` -- Windows Terminal owns `Ctrl+V` and never lets the
//!   app see it, feeding the clipboard in afterward as simulated
//!   keystrokes at ~7-8ms each; the physical key is polled the same way
//!   so the paste can happen at once instead.
//! - `paste_flood` -- `PasteFlood`: what to do about that keystroke
//!   flood once this app has already pasted by itself (swallow it,
//!   and cope with the start of it arriving before the key press was
//!   seen).
//!
//! Other Windows-specific code that *isn't* about the terminal's own
//! input handling stays where it belongs -- `cmd.exe` quoting and App
//! Paths lookup (`command_line::browsing::shell_exec`), drive
//! enumeration (`explorer::drive_menu`), shell profiles
//! (`command_line::shell`).

#[cfg(windows)]
pub mod alt_key;
mod paste_flood;
#[cfg(windows)]
mod paste_hotkey;

pub use paste_flood::PasteFlood;
