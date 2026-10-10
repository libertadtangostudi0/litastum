//! Workarounds for how Windows Terminal and the console input API deliver
//! input, kept together on request:
//! - `paste_hotkey`: Windows Terminal owns `Ctrl+V` and types the
//!   clipboard in at ~7-8ms per character, so the key is polled and the
//!   paste done at once;
//! - `paste_flood`: swallowing that keystroke flood afterward;
//! - `zoom_keys`: Windows Terminal owns its zoom keys too, so a zoom per
//!   screen polls and presses them (`crate::terminal_zoom`).
//!
//! Other Windows code that isn't about terminal input stays in its own
//! module (`cmd.exe` quoting, App Paths, drives, shell profiles).

mod paste_flood;
#[cfg(windows)]
mod paste_hotkey;
#[cfg(windows)]
pub mod zoom_keys;

pub use paste_flood::PasteFlood;
