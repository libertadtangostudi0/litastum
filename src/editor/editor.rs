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


    /// `Ctrl+Shift+Left`/`Right` -- word-wise selection. Not part of
    /// `input`'s own dispatch (`editor_keymap.rs::handle_editor_key`
    /// calls this directly instead) -- see `bindings::extend_word_selection`'s
    /// own doc comment for why this needed real logic of its own rather
    /// than another entry in `standard_key_handler`'s declarative table.
    pub fn extend_word_selection(&mut self, forward: bool) {
        super::bindings::extend_word_selection(&mut self.state, forward);
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

        let selection_style = Style::default().fg(theme.text).bg(theme.current_row_bg);

        let mut editor_theme = EditorTheme::default()
            .base(Style::default().fg(theme.text).bg(theme.bg))
            .block(
                Block::bordered()
                    .border_style(Style::default().fg(theme.accent))
                    .title(self.path.to_string_lossy().into_owned()),
            )
            .selection_style(selection_style)
            .hide_status_line()
            // Absolute line numbers, themed to match the rest of the
            // chrome (edtui's own default is a hardcoded black/gray
            // gutter, unrelated to whatever scheme is active) rather
            // than relative — this is a general-purpose text editor,
            // not a modal vim-style one where relative numbers help
            // with `dj`/`5k`-style motions.
            .line_numbers_style(Style::default().fg(theme.text_dim).bg(theme.bg));

        // edtui paints the cursor's own cell *after* selection styling
        // (`EditorView::render`), unconditionally overwriting whatever
        // color was there -- `.hide_cursor()` only changes that overwrite
        // to `base` instead of leaving it alone, it doesn't skip it. Since
        // this keymap always keeps `state.cursor` exactly on the
        // selection's live end (see `bindings::extend_word_selection`'s
        // doc comment), that one cell is the last character of an active
        // selection -- painting it `base` made it visually look
        // unselected even though it's genuinely included in what `Copy`
        // grabs, reported directly as pasted text having one more
        // character than what looked highlighted. Painting it
        // `selection_style` instead, only while a selection is actually
        // active, keeps the visible highlight and the real selection in
        // agreement. With no selection, `hide_cursor()`'s usual `base` is
        // right: the real terminal cursor (a thin bar — see
        // `setup_terminal` in main.rs) is what should be visible there,
        // not edtui's own solid reverse-video block.
        editor_theme = if self.state.selection.is_some() {
            editor_theme.cursor_style(selection_style)
        } else {
            editor_theme.hide_cursor()
        };

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
    ///
    /// While extending a selection *forward* (growing rightward/downward
    /// from where it started), shifted one column *right* of
    /// `state.cursor`'s own cell -- `state.cursor` sits exactly on the
    /// selection's live end (see `bindings::extend_word_selection`'s
    /// doc comment for why that invariant is load-bearing), but a
    /// terminal's own bar-shaped cursor is drawn at the *left* edge of
    /// whatever cell it's positioned on. Left unshifted, the bar renders
    /// on the boundary between the last selected character and the one
    /// before it -- reads as "the selection stops one character early"
    /// even though the highlighted cell and what `Copy` grabs are both
    /// already correct (confirmed directly: real logs showed the last
    /// selected character's cell genuinely painted with the selection
    /// color and genuinely included in the copied text -- only the
    /// blinking bar's own screen position was misleading). Shifting it
    /// one column right puts the bar on the boundary *after* the last
    /// selected character instead, matching where a caret sits at the
    /// end of a selection in every other editor.
    ///
    /// While extending *backward* (growing leftward/upward), the cursor
    /// sits at the *earliest* end of the selection instead, not the
    /// latest -- shifting right there put the bar on the boundary
    /// *after* the first selected character rather than before it,
    /// reported directly against real text (retracting onto the `'l'`
    /// of "loaded" rendered the bar between `'l'` and `'o'`, reading as
    /// if `'l'` itself weren't selected, even though it genuinely was).
    /// So the shift only applies when the cursor is at or after the
    /// selection's own `start` in reading order (row, then column) --
    /// i.e. only while it's the *trailing* edge of the selection, which
    /// is exactly the forward-extension case above. Plain typing (no
    /// selection) is unaffected either way -- the cursor already sits
    /// exactly where the next typed character would land, no shift
    /// needed there.
    pub fn cursor_screen_position(&self) -> Option<ratatui::layout::Position> {
        let mut pos = self.state.cursor_screen_position()?;
        if let Some(selection) = &self.state.selection {
            let cursor_is_trailing_edge = (self.state.cursor.row, self.state.cursor.col) >= (selection.start.row, selection.start.col);
            if cursor_is_trailing_edge {
                pos.x = pos.x.saturating_add(1);
            }
        }
        Some(pos)
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

    /// Regression test for a real bug: `edtui` paints the cursor's own
    /// cell *after* selection styling (`EditorView::render`),
    /// unconditionally overwriting whatever color was there — even under
    /// `.hide_cursor()`, which just repaints it as `base` rather than
    /// leaving it alone. Since this keymap always keeps `state.cursor`
    /// exactly on the selection's live end, that cell is the last
    /// character of an active selection: left unfixed, it visually looks
    /// unselected even though `Copy` genuinely includes it — reported
    /// directly as pasted text having one more character than what
    /// looked highlighted. `Editor::view` now paints the cursor cell with
    /// `selection_style` whenever a selection is active, so this checks
    /// that the fix actually lands where it's rendered, not just that the
    /// selection's own data is correct (which was never the bug).
    #[test]
    fn selection_end_cell_renders_with_selection_color_not_base() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (mut editor, _path) = open_test_editor("chat + the end");
        for _ in 0..7 {
            editor.input(key(KeyCode::Right));
        }
        editor.extend_word_selection(false); // Ctrl+Shift+Left

        let theme = Theme::dark();
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let view = editor.view(&theme);
                frame.render_widget(view, frame.area());
            })
            .unwrap();

        // This is a *backward* selection (`Ctrl+Shift+Left`) -- the
        // cursor sits at its earliest end, not its trailing one, so
        // `cursor_screen_position()` reports it unshifted now (see that
        // method's own doc comment): it already points straight at the
        // selected character's own cell, no stepping back needed.
        let cursor_pos = editor
            .cursor_screen_position()
            .expect("cursor should be visible after rendering");
        let selected_cell_x = cursor_pos.x;
        let buf = terminal.backend().buffer();
        let cell_bg = buf[(selected_cell_x, cursor_pos.y)].bg;

        assert_eq!(
            cell_bg, theme.current_row_bg,
            "the selection's own end -- where the cursor sits -- must render with the \
             selection color, not be reset to the base background by edtui's cursor-cell paint"
        );
    }

    /// Regression test for a real report: the selection's own end cell
    /// renders correctly (see the test above), but the real terminal's
    /// own blinking bar cursor is drawn at the *left* edge of whatever
    /// cell `cursor_screen_position()` reports -- left unshifted, that
    /// put the bar on the boundary *before* the last selected character
    /// rather than after it, reading as "the selection stopped one
    /// character early" even though the data (and the cell's own color)
    /// were already correct. `cursor_screen_position()` now shifts one
    /// column right whenever a selection is active, so the bar lands on
    /// the boundary *after* the last selected character instead.
    #[test]
    fn cursor_screen_position_is_shifted_past_the_selection_end_while_selecting() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (mut editor, _path) = open_test_editor("hello world");

        let theme = Theme::dark();
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let view = editor.view(&theme);
                frame.render_widget(view, frame.area());
            })
            .unwrap();
        let no_selection_pos = editor.cursor_screen_position().expect("cursor should be visible");

        editor.extend_word_selection(true); // Ctrl+Shift+Right, selects "hello"
        terminal
            .draw(|frame| {
                let view = editor.view(&theme);
                frame.render_widget(view, frame.area());
            })
            .unwrap();
        let with_selection_pos = editor.cursor_screen_position().expect("cursor should be visible");

        assert_eq!(
            with_selection_pos.x,
            no_selection_pos.x + 4 + 1,
            "cursor screen x should land one column past \"hello\"'s own last letter (index 4) while selecting"
        );
    }

    /// Regression test for a real, second report on the same underlying
    /// mechanism as the test above: the +1 shift is only correct while
    /// extending a selection *forward* (cursor at its trailing edge) --
    /// applying it unconditionally also shifted *backward* selections,
    /// whose cursor sits at the selection's *earliest* edge instead.
    /// Reported directly against real text ("loaded the"): a plain
    /// `Ctrl+Right` landing on the `'t'` of "the", followed by
    /// `Ctrl+Shift+Left`, retracted the cursor onto the `'l'` of
    /// "loaded" -- but the bar rendered one column too far right,
    /// between `'l'` and `'o'`, reading as if `'l'` itself weren't part
    /// of the selection even though it genuinely was.
    #[test]
    fn cursor_screen_position_is_not_shifted_for_a_backward_selection() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (mut editor, _path) = open_test_editor("loaded the file");

        let theme = Theme::dark();
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let view = editor.view(&theme);
                frame.render_widget(view, frame.area());
            })
            .unwrap();
        let cursor_on_l = editor.cursor_screen_position().expect("cursor should be visible");

        for _ in 0..7 {
            editor.input(key(KeyCode::Right)); // lands right on the 't' of "the"
        }
        editor.extend_word_selection(false); // Ctrl+Shift+Left, retracts onto "loaded"'s own 'l'
        terminal
            .draw(|frame| {
                let view = editor.view(&theme);
                frame.render_widget(view, frame.area());
            })
            .unwrap();
        let with_selection_pos = editor.cursor_screen_position().expect("cursor should be visible");

        assert_eq!(
            with_selection_pos.x, cursor_on_l.x,
            "cursor screen x should land exactly on 'l', not one column past it, while retracting a backward selection"
        );
    }
}
