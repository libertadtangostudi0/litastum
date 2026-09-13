//! Shared helpers for `#[cfg(test)]` modules across the crate — pulled
//! out after the same unique-scratch-directory and `KeyEvent`-builder
//! patterns turned up duplicated near-verbatim in over a dozen files'
//! own test modules. `#[cfg(test)]`-only (declared that way in
//! `main.rs`), so this compiles solely under `cargo test`, same as the
//! test code that uses it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::theming::Theme;

/// Removes every leftover `litastum-*-test-*` scratch directory under
/// the OS temp dir, once per test-binary run -- these are never cleaned
/// up individually after the test that created one finishes (there's no
/// teardown hook to hang that off), so left alone they accumulate
/// forever; found directly at over 69,000 entries under `%TEMP%`/
/// `W:\Temp` from past sessions. Beyond the wasted disk space, this
/// pile is *why* `unique_scratch_dir`'s own PID-based uniqueness could
/// still collide in practice: Windows reuses a process ID once it
/// exits, and with tens of thousands of old runs' directories sitting
/// around, a fresh `cargo test` process was likely enough to draw a PID
/// that matched one of them, landing a "fresh" scratch dir on top of
/// that old run's own leftover files (see `unique_scratch_dir`'s own
/// nanosecond-timestamp fix for the direct symptom this caused).
/// Requested directly, as the proper fix instead of a one-off manual
/// deletion: sweep the app's own leftovers under the OS temp dir on
/// every test run, rather than the whole temp dir.
///
/// `Once` ensures this runs exactly once per test binary and, just as
/// importantly, that every other test thread calling `unique_scratch_dir`
/// concurrently *blocks* until this sweep finishes before creating its
/// own directory -- without that, a thread could create its own fresh
/// scratch dir at the exact moment this function's own directory
/// listing was mid-iteration, and have it deleted out from under it.
/// Best-effort and silent on failure (a directory in use by another,
/// genuinely concurrent `cargo test` process elsewhere -- not
/// impossible, just not this codebase's normal workflow) --
/// `remove_dir_all` errors are swallowed rather than panicking a test
/// run over what's fundamentally housekeeping, not correctness.
fn cleanup_stale_scratch_dirs() {
    static CLEANED: Once = Once::new();
    CLEANED.call_once(|| {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("litastum-") && name.contains("-test-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
}

/// A fresh, unique scratch directory under the OS temp dir, already
/// created. `prefix` distinguishes one caller's directories from
/// another's when eyeballing the OS temp dir while debugging a failure
/// — the uniqueness itself comes from an atomic counter plus this
/// process's PID (`cargo test` runs tests in parallel threads within
/// one process, so the PID alone isn't enough) *and* a nanosecond
/// timestamp.
///
/// The timestamp is load-bearing, not decorative: without it, a fresh
/// `cargo test` run whose PID happens to match an old run's PID (a real
/// risk before `cleanup_stale_scratch_dirs` above existed -- Windows
/// reuses process IDs, and old runs' directories used to just pile up
/// forever) would generate the *exact same* directory names (the
/// counter always restarts at `0`), landing every "fresh" scratch dir
/// on top of that old run's leftover files instead of an empty
/// directory -- confirmed as the real cause of a batch of otherwise-
/// inexplicable failures (`AlreadyExists` creating a directory that was
/// supposedly just made, a search/listing test finding an extra
/// leftover file it never wrote).
pub fn unique_scratch_dir(prefix: &str) -> PathBuf {
    cleanup_stale_scratch_dirs();
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("litastum-{prefix}-test-{}-{n}-{nanos}", std::process::id()));
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

/// A real `App` (no terminal needed — `App::new` just wants a
/// directory) rooted at `dir`, with the dark built-in theme and no
/// custom syntax theme — the common case across every test that needs
/// an `App` but doesn't care which theme it has.
pub fn test_app(dir: PathBuf) -> App {
    App::new(dir, Theme::dark(), None).expect("build app")
}
