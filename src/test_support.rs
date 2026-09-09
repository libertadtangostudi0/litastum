//! Shared helpers for `#[cfg(test)]` modules across the crate — pulled
//! out after the same unique-scratch-directory and `KeyEvent`-builder
//! patterns turned up duplicated near-verbatim in over a dozen files'
//! own test modules. `#[cfg(test)]`-only (declared that way in
//! `main.rs`), so this compiles solely under `cargo test`, same as the
//! test code that uses it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::theming::Theme;

/// A fresh, unique scratch directory under the OS temp dir, already
/// created. `prefix` distinguishes one caller's directories from
/// another's when eyeballing the OS temp dir while debugging a failure
/// — the uniqueness itself comes from an atomic counter plus this
/// process's PID (`cargo test` runs tests in parallel threads within
/// one process, so the PID alone isn't enough).
pub fn unique_scratch_dir(prefix: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("litastum-{prefix}-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// A plain, unmodified key press.
pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// A `Ctrl`-held character key press.
pub fn ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// A `Ctrl`-held key press for a non-character key (e.g. `Ctrl+Left`) —
/// `ctrl_key` above is for a `Ctrl`-held *character* shortcut instead.
pub fn ctrl_code_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

/// A `Shift`-held key press.
pub fn shift_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

/// A `Ctrl+Shift`-held key press.
pub fn ctrl_shift_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
}

/// A real `App` (no terminal needed — `App::new` just wants a
/// directory) rooted at `dir`, with the dark built-in theme and no
/// custom syntax theme — the common case across every test that needs
/// an `App` but doesn't care which theme it has.
pub fn test_app(dir: PathBuf) -> App {
    App::new(dir, Theme::dark(), None).expect("build app")
}
