//! Workarounds for how Windows Terminal and the console input API deliver
//! input, kept together on request:
//! - `alt_key`: a bare `Alt` produces no event, so its state is polled;
//! - `paste_hotkey`: Windows Terminal owns `Ctrl+V` and types the
//!   clipboard in at ~7-8ms per character, so the key is polled and the
//!   paste done at once;
//! - `paste_flood`: swallowing that keystroke flood afterward.
//!
//! Other Windows code that isn't about terminal input stays in its own
//! module (`cmd.exe` quoting, App Paths, drives, shell profiles).

#[cfg(windows)]
pub mod alt_key;
mod paste_flood;
#[cfg(windows)]
mod paste_hotkey;

pub use paste_flood::PasteFlood;
