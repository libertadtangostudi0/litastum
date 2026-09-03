use std::fs;
use std::io;
use std::path::PathBuf;

use arboard::Clipboard;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use tracing::{debug, warn};
use tui_textarea::{Input, TextArea};

use crate::theme::Theme;


/// A single open-file editing session, backed by `tui-textarea`. Owns
/// the path it was loaded from, so `save` knows where to write back.
pub struct Editor {
    path: PathBuf,
    textarea: TextArea<'static>,
    dirty: bool,
}


impl Editor {
    /// Loads `path`'s contents into a new editing session, styled to
    /// match `theme`. Fails if the file can't be read as UTF-8 text
    /// (binary files aren't supported yet — see `TODO.md`).
    pub fn open(path: PathBuf, theme: &Theme) -> io::Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let lines: Vec<String> = contents.lines().map(str::to_string).collect();
        let lines = if lines.is_empty() { vec![String::new()] } else { lines };

        let mut textarea = TextArea::new(lines);
        textarea.set_style(Style::default().fg(theme.text).bg(theme.bg));
        textarea.set_cursor_line_style(Style::default().bg(theme.current_row_bg));
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title(path.to_string_lossy().into_owned()),
        );

        Ok(Self { path, textarea, dirty: false })
    }


    /// Feeds one input event to the underlying text area.
    pub fn input(&mut self, input: impl Into<Input>) {
        if self.textarea.input(input) {
            self.dirty = true;
        }
    }


    /// Copies the current selection to the OS clipboard (so it can be
    /// pasted into another application), as well as `tui-textarea`'s
    /// own internal yank buffer.
    ///
    /// `tui-textarea`'s built-in `Ctrl+C` only reaches the internal
    /// buffer, invisible outside this app — the caller (`main.rs`)
    /// intercepts `Ctrl+C` before it reaches `TextArea::input` and
    /// routes it here instead.
    pub fn copy(&mut self) {
        self.textarea.copy();
        let text = self.textarea.yank_text();
        debug!(chars = text.chars().count(), "editor copy");
        set_clipboard_text(text);
    }


    /// Cuts the current selection (deletes it, keeping a copy) to the
    /// OS clipboard and the internal yank buffer, mirroring `copy`.
    pub fn cut(&mut self) {
        let modified = self.textarea.cut();
        if modified {
            self.dirty = true;
        }
        let text = self.textarea.yank_text();
        debug!(modified, chars = text.chars().count(), "editor cut");
        set_clipboard_text(text);
    }


    /// Pastes at the cursor, preferring the OS clipboard's text over
    /// `tui-textarea`'s internal yank buffer — falls back to the
    /// internal buffer only when the OS clipboard is empty or
    /// unavailable (e.g. a prior in-app cut with no OS clipboard
    /// access on this platform/session).
    ///
    /// `tui-textarea`'s own default keymap is Emacs-style: `Ctrl+V` is
    /// bound to "scroll down a page", not paste (paste is `Ctrl+Y`
    /// there). We want the OS/VSCode convention instead, so `Ctrl+V` is
    /// intercepted by the caller (`main.rs`) before it reaches
    /// `TextArea::input`, and routed here.
    pub fn paste(&mut self) {
        self.paste_with(clipboard_text());
    }


    /// The actual paste decision, taking the OS clipboard's text (or
    /// its absence) as a parameter rather than reading it directly —
    /// kept separate from `paste` so this logic can be tested without
    /// touching the real OS clipboard (which would be both flaky in CI
    /// and rude to whatever the developer running the tests had
    /// copied).
    fn paste_with(&mut self, external_text: Option<String>) {
        let modified = match external_text {
            Some(text) if !text.is_empty() => {
                debug!(chars = text.chars().count(), "editor paste (external)");
                self.textarea.insert_str(text)
            }
            _ => {
                debug!("editor paste (internal yank buffer fallback)");
                self.textarea.paste()
            }
        };
        if modified {
            self.dirty = true;
        }
    }


    /// Writes the current buffer back to the file it was opened from.
    pub fn save(&mut self) -> io::Result<()> {
        let mut contents = self.textarea.lines().join("\n");
        contents.push('\n');
        debug!(path = %self.path.display(), bytes = contents.len(), "editor save: writing");
        match fs::write(&self.path, contents) {
            Ok(()) => {
                self.dirty = false;
                debug!(path = %self.path.display(), "editor save: ok");
                Ok(())
            }
            Err(err) => {
                warn!(path = %self.path.display(), %err, "editor save: failed");
                Err(err)
            }
        }
    }


    /// Whether the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }


    /// The renderable widget for this editing session.
    pub fn widget(&self) -> &TextArea<'static> {
        &self.textarea
    }
}


/// Best-effort write to the OS clipboard. Silently does nothing if no
/// clipboard is available (headless session, permissions, ...) or the
/// text is empty — there's no status bar yet to surface a failure to.
fn set_clipboard_text(text: String) {
    if text.is_empty() {
        return;
    }
    match Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(text) {
            Ok(()) => debug!("clipboard: set ok"),
            Err(err) => warn!(%err, "clipboard: set_text failed"),
        },
        Err(err) => warn!(%err, "clipboard: unavailable (Clipboard::new failed)"),
    }
}


/// Best-effort read from the OS clipboard. Returns `None` if no
/// clipboard is available or it holds no text.
fn clipboard_text() -> Option<String> {
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(err) => {
            warn!(%err, "clipboard: unavailable (Clipboard::new failed)");
            return None;
        }
    };
    match clipboard.get_text() {
        Ok(text) => {
            debug!(chars = text.chars().count(), "clipboard: get_text ok");
            Some(text)
        }
        Err(err) => {
            debug!(%err, "clipboard: get_text failed (likely empty or non-text)");
            None
        }
    }
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tui_textarea::Key;

    use super::*;

    /// Writes `contents` to a scratch file and opens it, so tests can
    /// exercise `Editor` without a fixture directory. Each call gets a
    /// distinct filename (`cargo test` runs tests in parallel threads
    /// within one process, so `process::id()` alone would collide).
    fn open_test_editor(contents: &str) -> Editor {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("litastum-editor-test-{}-{n}.txt", std::process::id()));
        fs::write(&path, contents).expect("write test fixture file");
        Editor::open(path, &Theme::dark()).expect("open test fixture file")
    }

    fn shift(key: Key) -> Input {
        Input { key, ctrl: false, alt: false, shift: true }
    }

    fn ctrl(key: Key) -> Input {
        Input { key, ctrl: true, alt: false, shift: false }
    }

    /// Regression test for a bug found by hand: `tui-textarea`'s default
    /// keymap binds `Ctrl+V` to page-down scrolling (Emacs-style), not
    /// paste, so pressing it silently did nothing useful instead of
    /// pasting. `Editor::paste` bypasses that binding — this exercises
    /// the deterministic half of it (`paste_with`) without touching the
    /// real OS clipboard.
    #[test]
    fn paste_with_prefers_given_external_text_over_internal_yank_buffer() {
        let mut editor = open_test_editor("hello world\n");
        editor.input(Input { key: Key::End, ctrl: false, alt: false, shift: false });

        editor.paste_with(Some("!".to_string()));

        assert_eq!(editor.widget().lines()[0], "hello world!");
        assert!(editor.is_dirty());
    }

    #[test]
    fn paste_with_falls_back_to_internal_yank_buffer_when_no_external_text() {
        let mut editor = open_test_editor("hello world\n");

        for _ in 0..5 {
            editor.input(shift(Key::Right)); // select "hello"
        }
        editor.input(ctrl(Key::Char('c'))); // tui-textarea's own internal copy
        editor.input(Input { key: Key::End, ctrl: false, alt: false, shift: false });

        editor.paste_with(None); // simulates an empty/unavailable OS clipboard

        assert_eq!(editor.widget().lines()[0], "hello worldhello");
        assert!(editor.is_dirty());
    }

    #[test]
    fn paste_with_empty_external_text_also_falls_back() {
        let mut editor = open_test_editor("hello world\n");

        for _ in 0..5 {
            editor.input(shift(Key::Right)); // select "hello"
        }
        editor.input(ctrl(Key::Char('c')));
        editor.input(Input { key: Key::End, ctrl: false, alt: false, shift: false });

        editor.paste_with(Some(String::new()));

        assert_eq!(editor.widget().lines()[0], "hello worldhello");
    }
}
