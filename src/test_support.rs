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

/// Removes leftover `litastum-*-test-*` scratch directories under the OS
/// temp dir, once per test binary -- nothing deletes them per test, and
/// 69,000 had piled up, making PID-based names collide. `Once` also blocks
/// other threads' `unique_scratch_dir` until the sweep is done, so a
/// fresh directory isn't deleted mid-listing. Best-effort: errors are
/// ignored. History: docs/history/tests.md.
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

/// A fresh scratch directory under the OS temp dir, named by `prefix`, the
/// PID, an atomic counter and a nanosecond timestamp. The timestamp
/// matters: Windows reuses PIDs and the counter restarts at 0, so a new
/// run could land on an old run's leftovers (`AlreadyExists`, extra files
/// in listings).
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


/// Every row of a rendered buffer, joined with newlines -- what a test
/// asserts against after drawing onto a `TestBackend`.
pub fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area;
    (0..area.height).map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
}


/// An `App` in `dir` with `contents` open in the built-in editor
/// (`keymap_mode`), as if `F4` had been pressed on it.
pub fn editing_app(contents: &str, keymap_mode: crate::editor::EditorKeymapMode) -> App {
    let dir = unique_scratch_dir("editing-app");
    let path = dir.join("file.txt");
    std::fs::write(&path, contents).expect("write test fixture file");
    let editor = crate::editor::Editor::open(path, None, keymap_mode).expect("open test fixture file");
    let mut app = test_app(dir);
    app.mode = crate::app::Mode::Editing(editor);
    app
}
