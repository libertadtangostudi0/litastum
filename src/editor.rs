use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use arboard::Clipboard as OsClipboard;
use crossterm::event::KeyEvent;
use edtui::actions::{
    Action, Chainable, CopySelection, DeleteChar, DeleteCharForward, DeleteSelection, LineBreak,
    MoveBackward, MoveDown, MoveForward, MoveHalfPageDown, MoveHalfPageUp, MoveToEndOfLine,
    MoveToStartOfLine, MoveUp, PasteBefore, Redo, SwitchMode, Undo,
};
use edtui::clipboard::ClipboardTrait;
use edtui::events::{KeyEventHandler, KeyEventRegister, KeyInput};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder};
use edtui::{
    EditorEventHandler, EditorMode, EditorState, EditorTheme, EditorView, LineNumbers, Lines, SyntaxHighlighter,
    SYNTAX_SET, THEME_SET,
};
use ratatui::style::Style;
use ratatui::widgets::Block;
use tracing::{debug, warn};

use crate::theme::Theme;

/// syntect theme bundled with `edtui`. `edtui`'s own docs are
/// inconsistent about naming here — `SyntaxHighlighter::theme`'s field
/// doc example says `"base16-ocean.dark"` (dot), but its own bundled
/// theme list a few lines below spells the same theme
/// `"base16-ocean-dark"` (hyphen) — the dotted form silently failed to
/// resolve (`SyntaxHighlighter::new` returned `Err`, so highlighting
/// was quietly skipped entirely). Using `"dracula"` instead: spelled
/// the same way everywhere in the crate's docs, so there's no similar
/// trap.
const SYNTAX_THEME: &str = "dracula";

/// `.sublime-syntax` (YAML) grammars for extensions `syntect`'s own
/// bundled default set (sourced from sublimehq/Packages) doesn't cover
/// at all — bundled at compile time via `include_str!`, licenses kept
/// alongside each in `assets/syntax/`:
///
/// - PowerShell (`.ps1`/`.psm1`/`.psd1`) — confirmed missing by
///   `syntect_bundles_rust_but_not_powershell` below. `syntect` only
///   loads the YAML `.sublime-syntax` format itself (its `plist-load`
///   feature is for `.tmTheme` *color themes*, not `.tmLanguage`
///   grammars — the obvious first choice, Microsoft's own
///   github.com/PowerShell/EditorSyntax, ships only a `.tmLanguage`
///   and turned out to be a dead end for that reason). This one is
///   from github.com/SublimeText/PowerShell (MIT license).
/// - INI (`.ini`/`.cfg`/`.conf`, and — via its own `hidden_file_extensions`
///   list — `.editorconfig` and a handful of other INI-shaped dotfiles)
///   — sublimehq/Packages has no INI syntax at all (checked directly,
///   not just syntect's build of it), so this isn't a syntect-specific
///   gap either. From github.com/jwortmann/ini-syntax (Apache-2.0
///   license).
/// - TOML (`.toml`, and `Cargo.lock`/`Gopkg.lock`/... via its own
///   `hidden_file_extensions`), Git Ignore (`.gitignore`), and Git
///   Attributes (`.gitattributes`) — all three genuinely present in
///   sublimehq/Packages (confirmed directly, browsing the repo) but,
///   unlike almost everything else there, apparently not included in
///   `syntect`'s own default bundle for some unknown reason. Same
///   permissive license as the rest of that repo
///   (`assets/syntax/sublimehq-Packages.LICENSE.txt`) — the exact
///   source `syntect`'s own default set is already built from, so
///   pulling a few more files from it raises no new licensing question.
///   Git Ignore and Git Attributes both `include:` rules from a shared
///   `Git Common.sublime-syntax` (`hidden: true` — not selectable by
///   extension on its own, only usable as an include target); it has
///   to be in this same `SyntaxSet` too or those includes silently
///   resolve to nothing and the file opens with no highlighting at all
///   (found by hand: `.gitignore` opened fine but rendered with zero
///   color, since a `SyntaxHighlighter` still resolved even when a
///   grammar's own internal includes don't — `syntect` doesn't treat
///   that as a load error).
/// - Git Config (`.gitconfig`/`.gitmodules` by name, and — via its own
///   `first_line_match: ^\[core\]` — plain `.git/config`, which has no
///   usable name or extension of its own at all). This is what pushed
///   `resolve_syntax_highlighter` below to add a first-line lookup
///   tier, not just name/extension: `.git/config` was never going to
///   be reachable any other way, and the grammar itself already
///   assumes that's how it'll be found.
const BUNDLED_GRAMMARS: &[&str] = &[
    include_str!("../assets/syntax/PowerShell.sublime-syntax"),
    include_str!("../assets/syntax/INI.sublime-syntax"),
    include_str!("../assets/syntax/TOML.sublime-syntax"),
    include_str!("../assets/syntax/GitCommon.sublime-syntax"),
    include_str!("../assets/syntax/GitIgnore.sublime-syntax"),
    include_str!("../assets/syntax/GitAttributes.sublime-syntax"),
    include_str!("../assets/syntax/GitConfig.sublime-syntax"),
];


/// A `SyntaxSet` containing just `BUNDLED_GRAMMARS` (not `edtui`'s own
/// shared default set — `syntect::parsing::SyntaxSet` isn't `Clone`, so
/// there's no cheap way to extend the one `edtui` already loaded;
/// building a second, minimal one just for these is simpler than
/// re-loading the entire default bundle a second time). Parsed once,
/// lazily, only if a file needing one of them is actually opened.
fn bundled_extra_syntax_set() -> &'static Arc<SyntaxSet> {
    static SET: OnceLock<Arc<SyntaxSet>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = SyntaxSetBuilder::new();
        for source in BUNDLED_GRAMMARS {
            match SyntaxDefinition::load_from_str(source, true, None) {
                Ok(syntax) => builder.add(syntax),
                Err(err) => warn!(%err, "failed to parse a bundled .sublime-syntax grammar"),
            }
        }
        Arc::new(builder.build())
    })
}


/// Resolves a `SyntaxHighlighter` for a file, general-purpose: tries
/// `candidates` (typically `[file_name, extension]`) against `syntect`'s
/// own bundled set, then our extra `BUNDLED_GRAMMARS`; if neither
/// matched by name at all, falls back to `first_line` against both sets
/// in the same order — matching `syntect`'s own convenience method
/// `SyntaxSet::find_syntax_for_file`'s two-tier lookup (name, then
/// first line), just spread across two `SyntaxSet`s instead of one.
///
/// The first-line tier exists specifically for files with no usable
/// name of their own — `.git/config` has neither a recognizable
/// extension nor (usually) any distinguishing part of its path, but
/// `GitConfig.sublime-syntax` declares `first_line_match: ^\[core\]`
/// for exactly this reason. Without this tier, *any* future grammar
/// that leans on first-line detection (shebang scripts, XML doctypes,
/// ...) would need its own one-off special case instead of just
/// working the way its own grammar file already says it should.
fn resolve_syntax_highlighter(candidates: &[&str], first_line: &str, custom_syntax_theme: &Option<SynTheme>) -> Option<SyntaxHighlighter> {
    let extra_set = bundled_extra_syntax_set();

    for candidate in candidates {
        if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_extension(candidate) {
            return build_highlighter(SYNTAX_SET.clone(), syntax_ref.clone(), custom_syntax_theme);
        }
    }
    for candidate in candidates {
        if let Some(syntax_ref) = extra_set.find_syntax_by_extension(candidate) {
            return build_highlighter(extra_set.clone(), syntax_ref.clone(), custom_syntax_theme);
        }
    }

    if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_first_line(first_line) {
        return build_highlighter(SYNTAX_SET.clone(), syntax_ref.clone(), custom_syntax_theme);
    }
    if let Some(syntax_ref) = extra_set.find_syntax_by_first_line(first_line) {
        return build_highlighter(extra_set.clone(), syntax_ref.clone(), custom_syntax_theme);
    }

    None
}


/// Builds a `SyntaxHighlighter` from an already-resolved `syntax_set`/
/// `syntax_ref` pair, applying `custom_syntax_theme` in place of the
/// named `SYNTAX_THEME` fallback if one is set. `None` only if
/// `SYNTAX_THEME` itself somehow isn't in `THEME_SET` (would mean the
/// bundled theme dump is broken, not a per-file lookup failure).
fn build_highlighter(syntax_set: Arc<SyntaxSet>, syntax_ref: SyntaxReference, custom_syntax_theme: &Option<SynTheme>) -> Option<SyntaxHighlighter> {
    let theme_set = THEME_SET.clone();
    let theme = match custom_syntax_theme {
        Some(custom) => custom.clone(),
        None => theme_set.themes.get(SYNTAX_THEME)?.clone(),
    };
    Some(SyntaxHighlighter::with_sets(theme, theme_set, syntax_ref, syntax_set))
}


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
        let candidates: Vec<&str> = [file_name, extension].into_iter().flatten().collect();

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


/// Bridges `edtui`'s pluggable clipboard trait to the real OS clipboard
/// via our own minimal `arboard` dependency (`default-features =
/// false`, so no `image` crate) — rather than enabling `edtui`'s own
/// `arboard` feature, which pulls `image`/`image-data` in for bitmap
/// clipboard support we don't need.
struct OsClipboardBridge;

impl ClipboardTrait for OsClipboardBridge {
    fn set_text(&mut self, text: String) {
        match OsClipboard::new() {
            Ok(mut clipboard) => match clipboard.set_text(text) {
                Ok(()) => debug!("clipboard: set ok"),
                Err(err) => warn!(%err, "clipboard: set_text failed"),
            },
            Err(err) => warn!(%err, "clipboard: unavailable (Clipboard::new failed)"),
        }
    }

    fn get_text(&mut self) -> String {
        match OsClipboard::new() {
            Ok(mut clipboard) => clipboard.get_text().unwrap_or_default(),
            Err(err) => {
                warn!(%err, "clipboard: unavailable (Clipboard::new failed)");
                String::new()
            }
        }
    }
}


/// A non-modal (VSCode/Windows-convention) keymap for `edtui`, which
/// ships only Vim and Emacs presets. `edtui` is explicitly designed for
/// this — `KeyEventHandler::new` takes any binding table — so this
/// isn't a workaround.
///
/// The editor stays in `EditorMode::Insert` for ordinary typing/
/// movement; `EditorMode::Visual` is entered only for the duration of a
/// `Shift+Arrow` selection and always exited back to `Insert` (never
/// left in `Normal`, which this keymap doesn't otherwise use).
fn standard_key_handler() -> KeyEventHandler {
    use crossterm::event::KeyCode;
    use std::collections::HashMap;

    /// Exits an active selection back to plain typing. Goes through
    /// `Normal` on the way, since `SwitchMode(Insert)` alone doesn't
    /// clear `state.selection` (only `SwitchMode(Normal)` does) and
    /// the view renders whatever `state.selection` holds regardless of
    /// mode — found while testing this integration.
    fn exit_selection() -> Action {
        SwitchMode(EditorMode::Normal).chain(SwitchMode(EditorMode::Insert)).into()
    }

    let i = |key: KeyInput| KeyEventRegister::i(vec![key]);
    let v = |key: KeyInput| KeyEventRegister::v(vec![key]);

    #[rustfmt::skip]
    let register: HashMap<KeyEventRegister, Action> = HashMap::from([
        // Plain movement, typing mode.
        (i(KeyInput::new(KeyCode::Left)), MoveBackward(1).into()),
        (i(KeyInput::new(KeyCode::Right)), MoveForward(1).into()),
        (i(KeyInput::new(KeyCode::Up)), MoveUp(1).into()),
        (i(KeyInput::new(KeyCode::Down)), MoveDown(1).into()),
        (i(KeyInput::new(KeyCode::Home)), MoveToStartOfLine().into()),
        (i(KeyInput::new(KeyCode::End)), MoveToEndOfLine().into()),
        (i(KeyInput::new(KeyCode::PageUp)), MoveHalfPageUp().into()),
        (i(KeyInput::new(KeyCode::PageDown)), MoveHalfPageDown().into()),

        // Shift+arrow starts (or extends) a selection.
        (i(KeyInput::shift(KeyCode::Left)), SwitchMode(EditorMode::Visual).chain(MoveBackward(1)).into()),
        (i(KeyInput::shift(KeyCode::Right)), SwitchMode(EditorMode::Visual).chain(MoveForward(1)).into()),
        (i(KeyInput::shift(KeyCode::Up)), SwitchMode(EditorMode::Visual).chain(MoveUp(1)).into()),
        (i(KeyInput::shift(KeyCode::Down)), SwitchMode(EditorMode::Visual).chain(MoveDown(1)).into()),
        (v(KeyInput::shift(KeyCode::Left)), MoveBackward(1).into()),
        (v(KeyInput::shift(KeyCode::Right)), MoveForward(1).into()),
        (v(KeyInput::shift(KeyCode::Up)), MoveUp(1).into()),
        (v(KeyInput::shift(KeyCode::Down)), MoveDown(1).into()),

        // Plain movement while a selection is active collapses it.
        (v(KeyInput::new(KeyCode::Left)), exit_selection().chain(MoveBackward(1)).into()),
        (v(KeyInput::new(KeyCode::Right)), exit_selection().chain(MoveForward(1)).into()),
        (v(KeyInput::new(KeyCode::Up)), exit_selection().chain(MoveUp(1)).into()),
        (v(KeyInput::new(KeyCode::Down)), exit_selection().chain(MoveDown(1)).into()),
        (v(KeyInput::new(KeyCode::Home)), exit_selection().chain(MoveToStartOfLine()).into()),
        (v(KeyInput::new(KeyCode::End)), exit_selection().chain(MoveToEndOfLine()).into()),
        (v(KeyInput::new(KeyCode::Esc)), exit_selection().into()),

        // Editing.
        (i(KeyInput::new(KeyCode::Backspace)), DeleteChar(1).into()),
        (i(KeyInput::new(KeyCode::Delete)), DeleteCharForward(1).into()),
        (i(KeyInput::new(KeyCode::Enter)), LineBreak(1).into()),
        (v(KeyInput::new(KeyCode::Backspace)), DeleteSelection.chain(exit_selection()).into()),
        (v(KeyInput::new(KeyCode::Delete)), DeleteSelection.chain(exit_selection()).into()),

        // Undo/redo (Windows/VSCode convention).
        (i(KeyInput::ctrl('z')), Undo.into()),
        (i(KeyInput::ctrl('y')), Redo.into()),

        // Clipboard. Copy/cut only make sense with a selection; paste
        // works from plain typing mode, and also exits a selection
        // first if one was active (simplification: this does not
        // replace the selection with the pasted text, just clears it
        // and pastes at the cursor — see TODO.md). `PasteBefore` (vim's
        // `P`) inserts exactly at the cursor; the plain `Paste` action
        // (vim's `p`) inserts *after* it instead, which felt wrong for
        // a "standard" editor — found while writing tests for this.
        (v(KeyInput::ctrl('c')), CopySelection.chain(exit_selection()).into()),
        (v(KeyInput::ctrl('x')), DeleteSelection.chain(exit_selection()).into()),
        (i(KeyInput::ctrl('v')), PasteBefore.into()),
        (v(KeyInput::ctrl('v')), exit_selection().chain(PasteBefore).into()),
    ]);

    // `capture_on_insert: true` -- take an undo checkpoint before every
    // typed character. `false` (the vim-mode default) relies on
    // `SwitchMode(Insert)` transitions to create checkpoints instead,
    // but this keymap sets `state.mode = Insert` once directly at open
    // and mostly stays there, so with `false` a plain typing session
    // created *zero* undo checkpoints -- Ctrl+Z was silently a no-op.
    // Found by a failing test, not by inspection. Per-character undo
    // granularity isn't as slick as grouping by typing burst, but
    // `EditorState::capture` is crate-private, so there's no hook to
    // implement that grouping ourselves.
    KeyEventHandler::new(register, true)
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crossterm::event::{KeyCode, KeyModifiers};
    use edtui::clipboard::InternalClipboard;

    use super::*;

    /// Writes `contents` to a scratch file and opens it, so tests can
    /// exercise `Editor` without a fixture directory. Each call gets a
    /// distinct filename (`cargo test` runs tests in parallel threads
    /// within one process, so `process::id()` alone would collide).
    /// Returns the path too, so tests can read back what `save` wrote.
    fn open_test_editor(contents: &str) -> (Editor, PathBuf) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("litastum-editor-test-{}-{n}.txt", std::process::id()));
        fs::write(&path, contents).expect("write test fixture file");
        let editor = Editor::open(path.clone(), None).expect("open test fixture file");
        (editor, path)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Pins down what's actually true about `syntect`'s bundled default
    /// syntax set, found by hand while debugging a "no highlighting for
    /// .ps1" report: `.rs` is bundled, `.ps1` (PowerShell) is not — not
    /// a bug in `Editor::view`'s extension lookup, an upstream gap, and
    /// the reason `custom_extension_highlighter`/`POWERSHELL_SYNTAX`
    /// exist at all. If `syntect` ever adds/drops one of these, this
    /// will fail and flag it rather than silently changing behavior.
    #[test]
    fn syntect_bundles_rust_but_not_powershell() {
        assert!(SyntaxHighlighter::new(SYNTAX_THEME, "rs").is_ok());
        assert!(SyntaxHighlighter::new(SYNTAX_THEME, "ps1").is_err());
    }

    /// The actual fix for the gap above: our own bundled
    /// `.sublime-syntax` grammar covers what `syntect`'s default set
    /// doesn't, for all three PowerShell extensions (case-insensitively,
    /// matching `find_syntax_by_extension`'s own behavior) — and stays
    /// `None` for something neither set has, rather than panicking.
    #[test]
    fn resolve_syntax_highlighter_covers_powershell_extensions() {
        assert!(resolve_syntax_highlighter(&["ps1"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["PS1"], "", &None).is_some(), "should match case-insensitively");
        assert!(resolve_syntax_highlighter(&["psm1"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["psd1"], "", &None).is_some());
        assert!(
            resolve_syntax_highlighter(&["rs"], "", &None).is_some(),
            "should still resolve syntect's own bundled grammars, not just ours"
        );
        assert!(resolve_syntax_highlighter(&["made-up-extension"], "", &None).is_none());
    }

    #[test]
    fn resolve_syntax_highlighter_covers_ini_extensions() {
        assert!(resolve_syntax_highlighter(&["ini"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["INI"], "", &None).is_some(), "should match case-insensitively");
        assert!(resolve_syntax_highlighter(&["cfg"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["conf"], "", &None).is_some());
    }

    /// Not a new grammar of its own — `INI.sublime-syntax`'s own
    /// `hidden_file_extensions` already lists `.editorconfig` (a full
    /// *file name*, not a bare extension), so this comes for free once
    /// the INI grammar was bundled for `.ini`/`.cfg`/`.conf`.
    #[test]
    fn resolve_syntax_highlighter_covers_editorconfig_via_the_ini_grammar() {
        assert!(resolve_syntax_highlighter(&[".editorconfig"], "", &None).is_some());
    }

    #[test]
    fn resolve_syntax_highlighter_covers_toml_and_git_formats() {
        assert!(resolve_syntax_highlighter(&["toml"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["TOML"], "", &None).is_some(), "should match case-insensitively");
        assert!(
            resolve_syntax_highlighter(&["Cargo.lock"], "", &None).is_some(),
            "TOML.sublime-syntax's own hidden_file_extensions covers this by full file name, not extension"
        );
        assert!(resolve_syntax_highlighter(&["gitignore"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["gitattributes"], "", &None).is_some());
    }

    #[test]
    fn resolve_syntax_highlighter_covers_gitconfig_by_name() {
        assert!(resolve_syntax_highlighter(&["gitconfig"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&[".gitconfig"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&[".gitmodules"], "", &None).is_some());
    }

    /// The actual point of the first-line lookup tier: `.git/config` has
    /// no usable name of its own — its `file_name` candidate is just
    /// `"config"`, which no grammar declares by name — but
    /// `GitConfig.sublime-syntax`'s own `first_line_match: ^\[core\]`
    /// makes it resolvable anyway once the first line is checked too.
    #[test]
    fn resolve_syntax_highlighter_finds_git_config_by_first_line_when_the_name_is_useless() {
        assert!(
            resolve_syntax_highlighter(&["config"], "", &None).is_none(),
            "sanity: bare 'config' shouldn't match anything by name alone"
        );
        assert!(resolve_syntax_highlighter(&["config"], "[core]", &None).is_some());
    }

    /// Regression test for a real bug found by hand after the fixes
    /// above shipped: `.gitignore` opened without a crash and name-based
    /// lookup returned `Some`, but the file rendered with *zero*
    /// color — Git Ignore's own grammar `include:`s rules from a
    /// separate `Git Common.sublime-syntax` (`hidden: true`) that
    /// hadn't been bundled alongside it, so every `include:` silently
    /// resolved to nothing. `syntect` doesn't treat an unresolved
    /// include as a load error, so a `SyntaxHighlighter` still resolved
    /// regardless — the only way to actually catch this is to run real
    /// highlighting and check it colors *something*, which
    /// `resolve_syntax_highlighter_covers_*` above doesn't do.
    #[test]
    fn gitignore_comments_are_actually_colored_not_just_resolvable() {
        use edtui::syntect::easy::HighlightLines;

        let syntax_set = bundled_extra_syntax_set();
        let syntax_ref = syntax_set.find_syntax_by_extension(".gitignore").expect("gitignore syntax should resolve");
        let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();

        let mut highlighter = HighlightLines::new(syntax_ref, &theme);
        let spans = highlighter.highlight_line("# a comment\n", syntax_set).expect("highlighting should succeed");

        let base_foreground = theme.settings.foreground.expect("theme should define a foreground");
        assert!(
            spans.iter().any(|(style, _)| style.foreground != base_foreground),
            "a comment line should get at least one span colored differently from plain \
             foreground -- if this fails, an `include:` in the bundled grammar isn't resolving \
             again (e.g. a missing shared/common .sublime-syntax dependency)"
        );
    }

    /// `Editor::view`'s actual fallback path, end to end: a bundled-
    /// grammar file gets a working `syntax_highlighter` from
    /// `EditorView`, not just from calling `custom_extension_highlighter`
    /// directly. Includes dotfiles (`.gitignore`/`.gitattributes`) to
    /// pin down the file-name-first lookup fix in `view()` itself —
    /// `Path::extension()` returns `None` for those, so before that fix
    /// they'd never even have reached a highlighter lookup at all, let
    /// alone a successful one.
    #[test]
    fn opening_a_bundled_grammar_file_gets_a_working_syntax_highlighter() {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let fixtures = [
            ("script.ps1", "Write-Host 'hi'\n"),
            ("settings.ini", "[section]\nkey=value\n"),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            (".gitignore", "/target\n"),
            (".gitattributes", "* text=auto\n"),
            // No usable name of its own (just "config") -- only
            // resolvable via the first-line lookup tier, see
            // `resolve_syntax_highlighter_finds_git_config_by_first_line_when_the_name_is_useless`.
            ("config", "[core]\n\trepositoryformatversion = 0\n"),
        ];
        for (filename, contents) in fixtures {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("litastum-editor-bundled-test-{}-{n}", std::process::id()));
            fs::create_dir_all(&dir).expect("create scratch dir");
            let path = dir.join(filename);
            fs::write(&path, contents).expect("write test fixture file");
            let mut editor = Editor::open(path, None).expect("open test fixture");

            // EditorView doesn't expose whether a highlighter ended up
            // attached, so this only proves `view()` doesn't panic
            // building one -- `resolve_syntax_highlighter_covers_*`
            // above is what actually pins down that it resolves to `Some`.
            let _ = editor.view(&Theme::dark());
        }
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

    /// Builds an `EditorState` + our custom keymap directly (bypassing
    /// `Editor::open`'s real-file / real-OS-clipboard setup) with
    /// `InternalClipboard`, so copy/cut/paste tests never touch the
    /// actual system clipboard -- that would be flaky in CI and rude to
    /// whatever the developer running the tests had copied.
    fn test_state(contents: &str) -> (EditorState, EditorEventHandler) {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.set_clipboard(InternalClipboard::default());
        (state, EditorEventHandler::new(standard_key_handler()))
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn typing_inserts_characters() {
        let (mut state, mut handler) = test_state("");
        handler.on_key_event(key(KeyCode::Char('h')), &mut state);
        handler.on_key_event(key(KeyCode::Char('i')), &mut state);
        assert_eq!(String::from(state.lines.clone()), "hi");
    }

    #[test]
    fn shift_right_starts_a_selection() {
        let (mut state, mut handler) = test_state("hello");
        handler.on_key_event(shift_key(KeyCode::Right), &mut state);
        assert_eq!(state.mode, EditorMode::Visual);
        assert!(state.selection.is_some());
    }

    #[test]
    fn select_copy_paste_roundtrip() {
        let (mut state, mut handler) = test_state("hello world");

        // edtui's selection is inclusive on both ends (vim-style), so
        // N shift-rights from col 0 selects N+1 characters, not N --
        // found the hard way when this test first failed with a
        // trailing space included in the copied text.
        for _ in 0..4 {
            handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello"
        }
        handler.on_key_event(ctrl_key('c'), &mut state);
        assert_eq!(state.mode, EditorMode::Insert, "copy should return to typing mode");
        assert!(state.selection.is_none());

        handler.on_key_event(key(KeyCode::End), &mut state);
        handler.on_key_event(ctrl_key('v'), &mut state);

        assert_eq!(String::from(state.lines.clone()), "hello worldhello");
    }

    #[test]
    fn ctrl_x_cuts_the_selection() {
        let (mut state, mut handler) = test_state("hello world");

        for _ in 0..5 {
            handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello " (inclusive selection, see above)
        }
        handler.on_key_event(ctrl_key('x'), &mut state);

        assert_eq!(String::from(state.lines.clone()), "world");
        assert_eq!(state.mode, EditorMode::Insert);

        handler.on_key_event(ctrl_key('v'), &mut state);
        assert_eq!(String::from(state.lines.clone()), "hello world");
    }

    #[test]
    fn esc_cancels_selection_and_returns_to_insert() {
        let (mut state, mut handler) = test_state("hello");
        handler.on_key_event(shift_key(KeyCode::Right), &mut state);
        assert_eq!(state.mode, EditorMode::Visual);

        handler.on_key_event(key(KeyCode::Esc), &mut state);

        assert_eq!(state.mode, EditorMode::Insert);
        assert!(state.selection.is_none());
    }

    #[test]
    fn ctrl_z_undoes_last_insert() {
        let (mut state, mut handler) = test_state("");
        handler.on_key_event(key(KeyCode::Char('x')), &mut state);
        assert_eq!(String::from(state.lines.clone()), "x");

        handler.on_key_event(ctrl_key('z'), &mut state);
        assert_eq!(String::from(state.lines.clone()), "");
    }
}
