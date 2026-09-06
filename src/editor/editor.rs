use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::KeyEvent;
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::{EditorEventHandler, EditorMode, EditorState, EditorTheme, EditorView, LineNumbers, Lines};
use ratatui::style::Style;
use ratatui::widgets::Block;
use tracing::{debug, warn};

use crate::theming::Theme;

use super::bindings::standard_key_handler;
use super::clipboard::OsClipboardBridge;
use super::syntax::resolve_syntax_highlighter;

/// A single open-file editing session, backed by `edtui`. Owns the path
/// it was loaded from (for `save`) and a snapshot of the content as of
/// the last load/save (for `is_dirty`, computed by comparing the
/// current buffer to it — simpler and more accurate than tracking a
/// hand-maintained dirty flag, since it self-corrects if the user
/// undoes their way back to a saved state).
pub struct Editor {
    path: PathBuf,
    state: EditorState,
    event_handler: EditorEventHandler,
    saved_snapshot: Lines,
    /// Overrides `SYNTAX_THEME`'s named lookup when the user has a
    /// custom color scheme configured — see `config::load_active_theme`
    /// and `.claude/rules/litastum-theming.md`.
    custom_syntax_theme: Option<SynTheme>,
    /// The file's first line as of `open()` — the input to
    /// `resolve_syntax_highlighter`'s first-line lookup tier (e.g.
    /// `.git/config`'s `^\[core\]`). Captured once at open rather than
    /// re-derived from the live buffer on every `view()` call: matches
    /// what a real first-line grammar detection is meant to see (the
    /// file as opened), and avoids re-flattening the jagged `Lines`
    /// buffer into a `String` every frame just to peek at row 0.
    first_line: String,
}


impl Editor {
    /// Loads `path`'s contents into a new editing session. Fails if the
    /// file can't be read as UTF-8 text (binary files aren't supported
    /// yet — see `TODO.md`). `custom_syntax_theme` is `None` for the
    /// built-in named syntax theme, or a scheme-derived theme when the
    /// user has a custom color scheme configured.
    pub fn open(path: PathBuf, custom_syntax_theme: Option<SynTheme>) -> io::Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let lines = Lines::from(contents.as_str());
        let first_line = contents.lines().next().unwrap_or("").to_string();

        let mut state = EditorState::new(lines.clone());
        state.mode = EditorMode::Insert;
        state.set_clipboard(OsClipboardBridge);

        Ok(Self {
            path,
            state,
            event_handler: EditorEventHandler::new(standard_key_handler()),
            saved_snapshot: lines,
            custom_syntax_theme,
            first_line,
        })
    }


    /// Feeds one key event to the editor. Standard (non-modal) editing
    /// bindings — see `standard_key_handler` — everything not bound
    /// there that's a plain character still inserts, since the editor
    /// stays in `EditorMode::Insert` outside an active selection.
    pub fn input(&mut self, key: KeyEvent) {
        self.event_handler.on_key_event(key, &mut self.state);
    }


    /// Whether there's an active text selection (used by the caller to
    /// decide whether `Esc` should cancel the selection or close the
    /// editor).
    pub fn has_selection(&self) -> bool {
        self.state.selection.is_some()
    }


    /// Writes the current buffer back to the file it was opened from.
    pub fn save(&mut self) -> io::Result<()> {
        let contents = String::from(self.state.lines.clone());
        debug!(path = %self.path.display(), bytes = contents.len(), "editor save: writing");
        match fs::write(&self.path, &contents) {
            Ok(()) => {
                self.saved_snapshot = self.state.lines.clone();
                debug!(path = %self.path.display(), "editor save: ok");
                Ok(())
            }
            Err(err) => {
                warn!(path = %self.path.display(), %err, "editor save: failed");
                Err(err)
            }
        }
    }


    /// Whether the buffer differs from the last loaded/saved snapshot.
    pub fn is_dirty(&self) -> bool {
        self.state.lines != self.saved_snapshot
    }


    /// Builds this frame's renderable view: the editor content plus
    /// syntax highlighting (best-effort — silently skipped if nothing
    /// recognizes this file, by name or by first line) and our theme.
    ///
    /// Takes `&mut self`, unlike a typical read-only render helper:
    /// `EditorView` tracks scroll position as part of rendering, so it
    /// needs write access to `EditorState` even just to draw.
    pub fn view(&mut self, theme: &Theme) -> EditorView<'_, '_> {
        let custom_syntax_theme = &self.custom_syntax_theme;

        // Try the full file name first, then just the extension —
        // `syntect`'s own convenience lookup (`SyntaxSet::find_syntax_for_file`)
        // does the same, and it matters for dotfiles like `.gitignore`/
        // `.editorconfig`: `Path::extension()` returns `None` for those
        // (Rust treats a leading dot with no further dot as "no
        // extension", not as a hidden file with an empty name).
        // `resolve_syntax_highlighter` falls back to `self.first_line`
        // for files with no usable name at all, like `.git/config`.
        let file_name = self.path.file_name().and_then(|n| n.to_str());
        let extension = self.path.extension().and_then(|e| e.to_str());
        let mut candidates: Vec<&str> = [file_name, extension].into_iter().flatten().collect();

        // A ".sdk" suffix on top of an otherwise-recognizable file name
        // is this project's own build-system convention for a template
        // that becomes the inner file once processed (e.g.
        // "CMakeLists.txt.sdk" is a CMake template) -- also try the
        // name/extension with that one suffix stripped, so these
        // templates get the same highlighting the real file would, on
        // top of (not instead of) the direct candidates above.
        let inner_name = file_name.and_then(|name| name.strip_suffix(".sdk"));
        if let Some(inner_name) = inner_name {
            candidates.push(inner_name);
            if let Some(inner_extension) = Path::new(inner_name).extension().and_then(|e| e.to_str()) {
                candidates.push(inner_extension);
            }
        }

        let syntax_highlighter = resolve_syntax_highlighter(&candidates, &self.first_line, custom_syntax_theme);
        debug!(?candidates, found = syntax_highlighter.is_some(), "syntax highlighter lookup");

        let editor_theme = EditorTheme::default()
            .base(Style::default().fg(theme.text).bg(theme.bg))
            .block(
                Block::bordered()
                    .border_style(Style::default().fg(theme.accent))
                    .title(self.path.to_string_lossy().into_owned()),
            )
            // The real terminal cursor (a thin bar — see `setup_terminal`
            // in main.rs) is what's visible instead; edtui's own cursor
            // is a solid reverse-video block over the character cell,
            // which read as an odd shape rather than a normal caret.
            .hide_cursor()
            .selection_style(Style::default().fg(theme.text).bg(theme.current_row_bg))
            .hide_status_line()
            // Absolute line numbers, themed to match the rest of the
            // chrome (edtui's own default is a hardcoded black/gray
            // gutter, unrelated to whatever scheme is active) rather
            // than relative — this is a general-purpose text editor,
            // not a modal vim-style one where relative numbers help
            // with `dj`/`5k`-style motions.
            .line_numbers_style(Style::default().fg(theme.text_dim).bg(theme.bg));

        EditorView::new(&mut self.state)
            .theme(editor_theme)
            .syntax_highlighter(syntax_highlighter)
            .line_numbers(LineNumbers::Absolute)
    }


    /// Where the real terminal cursor should be positioned to sit on
    /// top of the character currently under edit — `None` if the
    /// cursor is currently scrolled out of view. Only meaningful after
    /// `view()` has actually been rendered this frame (it computes this
    /// as part of rendering).
    pub fn cursor_screen_position(&self) -> Option<ratatui::layout::Position> {
        self.state.cursor_screen_position()
    }
}


#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::test_support::{key, unique_scratch_dir};

    /// Writes `contents` to a scratch file and opens it, so tests can
    /// exercise `Editor` without a fixture directory. Returns the path
    /// too, so tests can read back what `save` wrote.
    fn open_test_editor(contents: &str) -> (Editor, PathBuf) {
        let path = unique_scratch_dir("editor").join("file.txt");
        fs::write(&path, contents).expect("write test fixture file");
        let editor = Editor::open(path.clone(), None).expect("open test fixture file");
        (editor, path)
    }

    #[test]
    fn open_starts_clean() {
        let (editor, _path) = open_test_editor("hello\n");
        assert!(!editor.is_dirty());
    }

    #[test]
    fn typing_marks_dirty() {
        let (mut editor, _path) = open_test_editor("hello\n");
        editor.input(key(KeyCode::Char('!')));
        assert!(editor.is_dirty());
    }

    #[test]
    fn save_writes_file_and_clears_dirty() {
        let (mut editor, path) = open_test_editor("hi\n");
        editor.input(key(KeyCode::Char('!')));
        editor.save().unwrap();

        assert!(!editor.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "!hi\n");
    }

    #[test]
    fn undo_after_save_makes_it_dirty_again() {
        // is_dirty compares against the saved snapshot rather than a
        // hand-maintained flag, so this should "just work" -- worth
        // pinning down as a test since it's the whole point of that design.
        let (mut editor, _path) = open_test_editor("hi\n");
        editor.input(key(KeyCode::Char('!')));
        editor.save().unwrap();
        assert!(!editor.is_dirty());

        editor.input(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert!(editor.is_dirty(), "undoing past the saved state should be dirty again");
    }
}
