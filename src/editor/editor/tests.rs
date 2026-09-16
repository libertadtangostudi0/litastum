use crossterm::event::{KeyCode, KeyModifiers};
use edtui::Index2;

use super::*;
use crate::test_support::{key, shift_key, unique_scratch_dir};

/// Writes `contents` to a scratch file and opens it, so tests can
/// exercise `Editor` without a fixture directory. Returns the path
/// too, so tests can read back what `save` wrote.
fn open_test_editor(contents: &str) -> (Editor, PathBuf) {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, contents).expect("write test fixture file");
    let editor = Editor::open(path.clone(), None, EditorKeymapMode::Standard).expect("open test fixture file");
    (editor, path)
}

/// Same as `open_test_editor`, but under a `.rs` name -- for tests that
/// need real syntax highlighting to actually be active (`file.txt`
/// above resolves to Plain Text, which colors nothing).
fn open_test_rust_editor(contents: &str) -> Editor {
    let path = unique_scratch_dir("editor").join("file.rs");
    fs::write(&path, contents).expect("write test fixture file");
    Editor::open(path, None, EditorKeymapMode::Standard).expect("open test fixture file")
}

#[test]
fn open_starts_clean() {
    let (editor, _path) = open_test_editor("hello\n");
    assert!(!editor.is_dirty());
}

/// `Standard` starts in `Insert` -- typing immediately inserts, matching
/// every non-modal editor this app is modeled on.
#[test]
fn opening_in_standard_mode_starts_in_insert() {
    let (editor, _path) = open_test_editor("hello\n");
    assert_eq!(editor.state.mode, EditorMode::Insert);
    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Standard);
}

/// `Vim` starts in `Normal`, matching real Vim's own convention and
/// `edtui`'s own `vim_mode()` binding table, which expects to begin
/// there (`starting_mode`'s own doc comment).
#[test]
fn opening_in_vim_mode_starts_in_normal() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "hello\n").expect("write test fixture file");
    let editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");

    assert_eq!(editor.state.mode, EditorMode::Normal);
    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Vim);
}

/// A real, behavioral difference between the two keymaps, not just a
/// data-field check: in `Vim`'s `Normal` mode, a plain letter is a
/// command, not literal text -- typing `x` there must *not* insert an
/// `x` into the buffer, unlike `Standard`'s own `Insert` mode where it
/// always does (`typing_marks_dirty` above).
#[test]
fn vim_normal_mode_does_not_insert_plain_characters() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "hello\n").expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");

    editor.input(key(KeyCode::Char('!')));

    assert!(!editor.is_dirty(), "a plain letter in Vim's Normal mode is a command, not inserted text");
}

/// `Editor::set_keymap_mode` live-switches an already-open session:
/// rebuilds the event handler, resets `state.mode` to the new keymap's
/// own starting point, and drops any active selection (neither
/// keymap's own selection semantics carry over meaningfully to the
/// other -- `set_keymap_mode`'s own doc comment).
#[test]
fn set_keymap_mode_switches_the_handler_mode_and_drops_any_selection() {
    let (mut editor, _path) = open_test_editor("hello\n");
    editor.input(shift_key(KeyCode::Right)); // opens a selection under Standard
    assert!(editor.has_selection(), "precondition: a selection should be active");

    editor.set_keymap_mode(EditorKeymapMode::Vim);

    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Vim);
    assert_eq!(editor.state.mode, EditorMode::Normal);
    assert!(!editor.has_selection());

    editor.set_keymap_mode(EditorKeymapMode::Standard);

    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Standard);
    assert_eq!(editor.state.mode, EditorMode::Insert);
}

/// Regression guard for the explicit request: this project's own
/// hand-rolled correction passes (`wrap_line_boundary_arrow_movement`,
/// `anchor_fresh_shift_selection`, ...) must not run at all once `Vim`
/// is active -- they were never tuned against Vim's own modal, multi-key
/// sequences (`EditorKeymapMode::Vim`'s own doc comment). Picks one
/// concrete, easily observed symptom of `wrap_line_boundary_arrow_movement`
/// actually running: under `Standard`, a plain `Left` at the very start
/// of a non-first line wraps up onto the end of the previous line. Under
/// `Vim`, `edtui`'s own `Normal`-mode `h` binding (not a raw `Left`
/// arrow at all) is what actually moves the cursor, and a raw `Left`
/// arrow simply isn't bound to anything in `edtui`'s own `vim_mode()`
/// table -- so it must have no effect, not even a wrap.
#[test]
fn line_boundary_wrapping_does_not_run_in_vim_mode() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "first\nsecond").expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");
    editor.state.cursor = Index2::new(1, 0); // start of the second line

    editor.input(key(KeyCode::Left));

    assert_eq!(editor.state.cursor, Index2::new(1, 0), "a raw Left arrow should have no effect in Vim mode, not wrap up a line");
}

#[test]
fn typing_marks_dirty() {
    let (mut editor, _path) = open_test_editor("hello\n");
    editor.input(key(KeyCode::Char('!')));
    assert!(editor.is_dirty());
}

/// Real, integration-level regression test for the reported bug
/// (`bindings/shift_select.rs`'s own doc comment has the full
/// story): one `Shift+Right` right before "Draft" must select
/// exactly "D", not "Dr". Goes through the real `Editor::input`
/// (not just the raw table in `bindings/mod.rs`'s own tests), since
/// this is exactly where the interaction with
/// `wrap_line_boundary_arrow_movement` matters -- a multi-line file
/// is used specifically so a wrongly-still-firing wrap check would
/// have jumped the selection down into the next line instead of
/// stopping on "D".
#[test]
fn shift_right_selects_exactly_one_character_mid_buffer() {
    let (mut editor, _path) = open_test_editor("Draft architecture\nsecond line");

    editor.input(shift_key(KeyCode::Right));

    let selection = editor.state.selection.expect("should have started a selection");
    assert_eq!(selection.start, selection.end, "exactly one cell should be selected");
    assert_eq!(editor.state.cursor, Index2 { row: 0, col: 0 }, "should not have wrapped to the next line or moved past 'D'");
}

/// Real, integration-level regression test for the retest that
/// caught `Shift+Left` selecting the character to the *right* of
/// the cursor instead of the left. Goes through the real
/// `Editor::input` for the same reason as the `Shift+Right` test
/// above -- confirms the wrap check is correctly skipped here too
/// (the anchor-drag already fully resolves this press).
#[test]
fn shift_left_selects_exactly_one_character_mid_buffer() {
    let (mut editor, _path) = open_test_editor("Draft architecture\nsecond line");
    editor.state.cursor.col = 6; // right before the 'a' of "architecture"

    editor.input(shift_key(KeyCode::Left));

    let selection = editor.state.selection.expect("should have started a selection");
    assert_eq!(selection.start, selection.end, "exactly one cell should be selected");
    assert_eq!(editor.state.cursor, Index2 { row: 0, col: 5 }, "should be on the space right before \"architecture\", not on 'a'");
}

/// Real, integration-level regression test for the reported bug
/// (`bindings/word_select.rs::extend_word_selection`'s own doc
/// comment, "Fourteenth", has the full story): a selection built
/// purely by walking backward (`Ctrl+Shift+Left` twice) must retrace
/// correctly when a `Ctrl+Shift+Right` follows -- undoing exactly the
/// most recent `Left`, not blindly extending forward from wherever the
/// cursor currently sits. Goes through the real `Editor::extend_word_selection`
/// (not the raw `bindings::extend_word_selection` directly), since this
/// is exactly where the bug lived: `WordSelectTouch` tracking never
/// recognized a forward press against a `NativeBackward`-built
/// selection as a retraction.
#[test]
fn ctrl_shift_right_retraces_a_backward_built_selection() {
    let (mut editor, _path) = open_test_editor("Draft architecture derived");
    editor.state.cursor.col = 18; // the space right after "architecture"

    editor.extend_word_selection(false); // Ctrl+Shift+Left, selects "architecture"
    editor.extend_word_selection(false); // Ctrl+Shift+Left, extends through "Draft" too
    editor.extend_word_selection(true); // Ctrl+Shift+Right, should undo the second press

    let selection = editor.state.selection.expect("should still have a selection");
    assert_eq!(selection.start.col, 17, "anchor should be back on 'e', the last letter of \"architecture\"");
    assert_eq!(selection.end.col, 6, "cursor should be back on 'a', the start of \"architecture\"");
    assert_eq!(editor.state.cursor.col, 6);
}

/// Continuing past a full retrace should close the selection entirely,
/// not leave a stray one-character selection sitting on the anchor --
/// see `retreat_forward_through_a_backward_walk`'s own doc comment for
/// why the crossing check has to be `>=`, not `==`, against the anchor.
#[test]
fn ctrl_shift_right_closes_the_selection_once_the_backward_walk_is_fully_retraced() {
    let (mut editor, _path) = open_test_editor("Draft architecture derived");
    editor.state.cursor.col = 18; // the space right after "architecture"

    editor.extend_word_selection(false); // "architecture"
    editor.extend_word_selection(false); // "Draft architecture"
    editor.extend_word_selection(true); // undoes "Draft", back to "architecture"
    editor.extend_word_selection(true); // undoes "architecture" too -- nothing left

    assert_eq!(editor.state.mode, EditorMode::Insert, "should have closed the selection entirely");
    assert!(editor.state.selection.is_none());
}

/// Real, integration-level regression test for the reported bug
/// (`bindings/word_select.rs::extend_word_selection`'s own doc
/// comment, "Sixteenth", has the full story): retracing a backward
/// selection must return the cursor to the exact column it started
/// at, not the trimmed anchor. Goes through the real `Editor::
/// extend_word_selection`, confirming `word_select_true_anchor` is
/// actually threaded through correctly end to end, not just in the
/// lower-level `bindings::extend_word_selection` unit tests.
#[test]
fn ctrl_shift_right_after_left_returns_to_the_true_starting_column() {
    let (mut editor, _path) = open_test_editor("derived");
    editor.state.cursor.col = 4; // between 'i' and 'v'

    editor.extend_word_selection(false); // Ctrl+Shift+Left, selects "deri"
    editor.extend_word_selection(true); // Ctrl+Shift+Right, should retrace back to column 4

    assert_eq!(editor.state.mode, EditorMode::Insert, "should have closed the selection entirely");
    assert!(editor.state.selection.is_none());
    assert_eq!(editor.state.cursor.col, 4, "should be back at the exact original column, not the trimmed anchor (3)");

    // One further Right starts a fresh forward selection from there,
    // matching VS Code's own "reflect" behavior for this same case.
    editor.extend_word_selection(true);
    let selection = editor.state.selection.expect("should have started a fresh forward selection");
    assert_eq!(selection.start.col, 4);
    assert_eq!(selection.end.col, 6, "should land on the last letter of \"derived\", selecting \"ved\"");
}

/// The line-boundary-crossing edge case: `Shift+Left` right at the
/// very start of a (non-first) line has nothing to select on that
/// line at all -- must still hand off to the wrap check and cross
/// into the previous line, the same way it already does for
/// `Shift+Right` at a line's own end
/// (`shift_right_selection_extends_across_the_line_boundary` in
/// `bindings/line_wrap.rs`), rather than silently doing nothing.
#[test]
fn shift_left_at_the_start_of_a_line_still_wraps_to_the_previous_line() {
    let (mut editor, _path) = open_test_editor("hi\nworld");
    editor.state.cursor = Index2 { row: 1, col: 0 };

    editor.input(shift_key(KeyCode::Left));

    assert_eq!(editor.state.cursor.row, 0, "should have crossed into the previous line");
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
    // `dirty` is recomputed by comparing against the saved snapshot on
    // every key that could plausibly have mutated the buffer
    // (`can_mutate_buffer`) -- Ctrl+Z is one of those, so this should
    // "just work" even though `dirty` is now a cached field rather than
    // a fresh comparison on every `is_dirty()` call (see `Editor::dirty`'s
    // own doc comment for why that changed).
    let (mut editor, _path) = open_test_editor("hi\n");
    editor.input(key(KeyCode::Char('!')));
    editor.save().unwrap();
    assert!(!editor.is_dirty());

    editor.input(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert!(editor.is_dirty(), "undoing past the saved state should be dirty again");
}

/// Regression test for the perf fix in `Editor::dirty`'s own doc
/// comment: navigation keys (`can_mutate_buffer` returns `false` for
/// them) must skip recomputing `dirty`, but the *cached* value from
/// before the navigation still has to come through correctly in both
/// directions -- a clean file must stay reported clean while merely
/// moving the cursor around, and a dirty one must stay reported dirty,
/// not accidentally reset by the skip.
#[test]
fn navigation_keys_never_change_the_cached_dirty_state() {
    let (mut editor, _path) = open_test_editor("hello world\nsecond line");
    assert!(!editor.is_dirty());

    for _ in 0..5 {
        editor.input(key(KeyCode::Right));
    }
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::Home));
    editor.input(key(KeyCode::End));
    assert!(!editor.is_dirty(), "pure navigation must not mark a clean file dirty");

    editor.input(key(KeyCode::Char('!')));
    assert!(editor.is_dirty());

    for _ in 0..5 {
        editor.input(key(KeyCode::Left));
    }
    assert!(editor.is_dirty(), "pure navigation must not clear a genuinely dirty file's flag");
}

/// Regression test for a real gap found by hand while testing Vim mode
/// against the same kind of file the fix above exists for: Vim's own
/// `h`/`j`/`k`/`l` navigation (not arrows) must get the identical
/// dirty-caching exemption, or a Vim user navigating a pathologically
/// long line with `hjkl` gets none of the perf fix's benefit at all
/// (`can_mutate_buffer`'s own doc comment has the full story, including
/// why `w`/`b`/`e`/etc. are deliberately *not* also exempted).
#[test]
fn vim_hjkl_navigation_never_changes_the_cached_dirty_state() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "hello world\nsecond line").expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");
    assert!(!editor.is_dirty());

    for _ in 0..5 {
        editor.input(key(KeyCode::Char('l')));
    }
    editor.input(key(KeyCode::Char('j')));
    editor.input(key(KeyCode::Char('h')));
    editor.input(key(KeyCode::Char('k')));
    assert!(!editor.is_dirty(), "pure hjkl navigation must not mark a clean file dirty");

    editor.input(key(KeyCode::Char('x'))); // deletes the character under the cursor
    assert!(editor.is_dirty(), "a genuine Vim edit must still be detected");

    for _ in 0..5 {
        editor.input(key(KeyCode::Char('l')));
    }
    assert!(editor.is_dirty(), "further hjkl navigation must not clear a genuinely dirty file's flag");
}

/// The exemption above must be Vim-specific -- under `Standard`, 'h'/
/// 'j'/'k'/'l' are ordinary letters that insert text, and must still be
/// correctly detected as edits (this pins down that the fix didn't
/// accidentally make the exemption keymap-independent).
#[test]
fn hjkl_still_marks_standard_mode_dirty_as_ordinary_typed_characters() {
    let (mut editor, _path) = open_test_editor("hello\n");
    assert!(!editor.is_dirty());

    for c in ['h', 'j', 'k', 'l'] {
        editor.input(key(KeyCode::Char(c)));
    }

    assert!(editor.is_dirty(), "hjkl typed in Standard mode are literal characters, not navigation");
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
            let view = editor.view(&theme, frame.area());
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

/// `Theme::selection_text`, when a scheme sets it (requested directly,
/// to keep text readable over a deliberately bright selection
/// background), overrides the selected text's own foreground -- plain
/// `theme.text` is the fallback (see the test above, which uses
/// `Theme::dark()`, where `selection_text` is `None`) only when a
/// scheme doesn't ask for this.
#[test]
fn selection_uses_the_theme_override_color_when_set() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut editor, _path) = open_test_editor("chat + the end");
    for _ in 0..7 {
        editor.input(key(KeyCode::Right));
    }
    editor.extend_word_selection(false); // Ctrl+Shift+Left

    let mut theme = Theme::dark();
    theme.selection_text = Some(ratatui::style::Color::Rgb(0, 0, 0));
    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    let cursor_pos = editor.cursor_screen_position().expect("cursor should be visible after rendering");
    let buf = terminal.backend().buffer();
    let cell_fg = buf[(cursor_pos.x, cursor_pos.y)].fg;
    assert_eq!(cell_fg, ratatui::style::Color::Rgb(0, 0, 0), "the selected text should use the theme's override color, not theme.text");
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
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let no_selection_pos = editor.cursor_screen_position().expect("cursor should be visible");

    editor.extend_word_selection(true); // Ctrl+Shift+Right, selects "hello"
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
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
            let view = editor.view(&theme, frame.area());
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
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let with_selection_pos = editor.cursor_screen_position().expect("cursor should be visible");

    assert_eq!(
        with_selection_pos.x, cursor_on_l.x,
        "cursor screen x should land exactly on 'l', not one column past it, while retracting a backward selection"
    );
}

/// Regression test for a real report: a file consisting of one
/// enormous line (an escaped log/diff dump, no real line breaks) made
/// the editor visibly sluggish -- `syntect` re-tokenizes a line's full
/// text on every highlight pass regardless of viewport, so this cost
/// was being paid fresh on every one of the app's per-event redraws.
/// `Editor::view` now skips building a `SyntaxHighlighter` at all for
/// such a file (`has_pathologically_long_line`) -- confirmed here by
/// rendering a `.rs` file (which does get real keyword coloring, see
/// the control case below) with one line padded well past
/// `word_highlight::MAX_HIGHLIGHTED_LINE_LEN`, and checking that no
/// rendered cell's foreground differs from the plain base text color.
#[test]
fn a_pathologically_long_line_disables_syntax_highlighting_for_the_whole_file() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let padding = "x".repeat(crate::editor::word_highlight::MAX_HIGHLIGHTED_LINE_LEN + 1);
    let mut editor = open_test_rust_editor(&format!("fn main() {{}} // {padding}"));

    let theme = Theme::dark();
    let backend = TestBackend::new(200, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let base_foreground = theme.text;
    assert!(
        buf.content()
            .iter()
            .all(|cell| cell.fg == base_foreground || cell.fg == theme.accent || cell.fg == theme.text_dim),
        "every cell should render in the plain base text color (border's own accent color, or \
         the line-number gutter's text_dim) once syntax highlighting is disabled for a \
         pathologically long line -- any other color means the syntax highlighter still ran"
    );
}

/// Control case for the test above: the same Rust content, short
/// enough to keep syntax highlighting active, genuinely does color at
/// least one cell (the `fn` keyword) differently from plain base text
/// -- confirms the assertion above is actually meaningful, not just
/// trivially true because `.rs` never gets colored in a `TestBackend`.
#[test]
fn a_short_rust_file_does_get_real_syntax_coloring() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut editor = open_test_rust_editor("fn main() {}");

    let theme = Theme::dark();
    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let base_foreground = theme.text;
    assert!(
        buf.content()
            .iter()
            .any(|cell| cell.fg != base_foreground && cell.fg != theme.accent && cell.fg != theme.text_dim),
        "the 'fn' keyword should be colored differently from plain base text/chrome"
    );
}

/// Real, rendering-level test for the bracket-matching feature
/// (`bracket_match.rs`): with the cursor on an opening bracket, its
/// closing partner should render in the same highlight style
/// `word_highlight.rs` uses (`theme.text` on `theme.border`,
/// `Editor::view` passes both passes the identical `Style` -- requested
/// directly, so brackets read as the same *kind* of hint as word
/// highlighting rather than a visually distinct feature); a plain
/// character between them should not. The cursor's *own* bracket isn't
/// asserted on directly -- `edtui` paints the cursor's own cell last,
/// on top of any highlight (`bracket_match_highlights`'s own doc
/// comment), so it never visibly carries the highlight style regardless
/// of whether this feature works at all.
#[test]
fn matching_brackets_render_in_the_shared_highlight_style() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut editor, _path) = open_test_editor("(a)");
    // Cursor starts at (0, 0), already on the '('.

    let theme = Theme::dark();
    let backend = TestBackend::new(20, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    // The real on-screen position of column 0 (the '(') -- content is
    // inset by the border and the line-number gutter, so this can't be
    // assumed to be (0, 0).
    let origin = editor.cursor_screen_position().expect("cursor should be visible");
    let buf = terminal.backend().buffer();
    let highlight_style = (theme.text, theme.border);
    let cell_style = |dx: u16| {
        let cell = &buf[(origin.x + dx, origin.y)];
        (cell.fg, cell.bg)
    };

    assert_eq!(cell_style(0), highlight_style, "the opening '(' under the cursor should also be highlighted");
    assert_eq!(cell_style(2), highlight_style, "the closing ')' should be highlighted");
    assert_ne!(cell_style(1), highlight_style, "the plain 'a' in between must not be highlighted");
}

/// Regression test for the explicit report (with a screenshot): once
/// `bracket_match_highlights` started returning both brackets of a
/// pair, the far one showed the highlight color but the *near* one --
/// wherever the cursor itself sat -- stayed plain, since `edtui` paints
/// the cursor's own cell last, silently overwriting any `Highlight`
/// there. Fixed via `Editor::view`'s `cursor_style` decision, now
/// painting that cell with `highlight_style` instead of `hide_cursor()`'s
/// plain `base` whenever `cursor_is_on_a_matched_bracket` says so. This
/// test pins the fix down directly by comparing the cursor's own cell
/// before and after moving onto a matched bracket -- it must change
/// color, not stay a constant `base`.
#[test]
fn the_bracket_under_the_cursor_is_also_visibly_highlighted() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut editor, _path) = open_test_editor("x(a)");
    // Cursor starts at (0, 0), on 'x' -- not touching a bracket at all.

    let theme = Theme::dark();
    let backend = TestBackend::new(20, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    let highlight_style = (theme.text, theme.border);
    let base_style = (theme.text, theme.bg);

    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let cursor_pos = editor.cursor_screen_position().expect("cursor should be visible");
    let buf = terminal.backend().buffer();
    let cell_at = |pos: ratatui::layout::Position| {
        let cell = &buf[(pos.x, pos.y)];
        (cell.fg, cell.bg)
    };
    assert_eq!(cell_at(cursor_pos), base_style, "'x' isn't a bracket, so the cursor cell should render plain");

    // Move onto the '(' at column 1.
    editor.input(key(KeyCode::Right));
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let cursor_pos = editor.cursor_screen_position().expect("cursor should be visible");
    let buf = terminal.backend().buffer();
    let cell_at = |pos: ratatui::layout::Position| {
        let cell = &buf[(pos.x, pos.y)];
        (cell.fg, cell.bg)
    };
    assert_eq!(cell_at(cursor_pos), highlight_style, "the '(' under the cursor should now render in the highlight color");
}

/// Regression test for the explicit request: bracket matching must
/// never feed into, or be fed by, word-occurrence highlighting -- the
/// two features stay fully independent, even though they now share one
/// color. With the cursor on the opening bracket, only the matching ')'
/// highlights (the word "foo" appearing twice must NOT light up, since
/// a bracket character was never a candidate for `word_highlight`'s own
/// "similar word" scan); moving the cursor onto "foo" flips this around
/// -- the *other* occurrence of the word highlights, and the bracket
/// highlight is gone entirely.
#[test]
fn bracket_matching_and_word_highlighting_never_interfere() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut editor, _path) = open_test_editor("(foo) foo");
    // Cursor starts at (0, 0), on the '('.

    let theme = Theme::dark();
    let backend = TestBackend::new(20, 3);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let origin = editor.cursor_screen_position().expect("cursor should be visible");
    let buf = terminal.backend().buffer();
    let highlight_style = (theme.text, theme.border);
    let cell_style = |dx: u16| {
        let cell = &buf[(origin.x + dx, origin.y)];
        (cell.fg, cell.bg)
    };

    assert_eq!(cell_style(4), highlight_style, "the matching ')' should be bracket-highlighted");
    assert_ne!(cell_style(1), highlight_style, "\"foo\" inside the parens must not get word-highlighted while the cursor sits on '('");
    assert_ne!(cell_style(6), highlight_style, "the second \"foo\" must not get word-highlighted either");

    // Move the cursor onto the middle of the first "foo" (column 2) --
    // not column 1, which still sits right after '(' and would count as
    // "touching" it, per the same convention `word_at` itself uses for
    // standing right after a word (see `bracket_at`'s own doc comment).
    editor.input(key(KeyCode::Right));
    editor.input(key(KeyCode::Right));

    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    let cell_style = |dx: u16| {
        let cell = &buf[(origin.x + dx, origin.y)];
        (cell.fg, cell.bg)
    };

    assert_eq!(cell_style(6), highlight_style, "the second \"foo\" should now be word-highlighted");
    assert_ne!(cell_style(4), highlight_style, "the ')' must not stay bracket-highlighted once the cursor left the '('");
}

/// Regression test for the reported bug: a multi-line matched bracket
/// pair only ever showed one bracket highlighted whenever the other
/// one had scrolled outside the currently-visible rows -- `edtui`'s own
/// vertical auto-scroll only ever keeps the *cursor's* row in view,
/// with no notion of "and this other row too." `Editor::view` now
/// widens the viewport to include the whole pair when it actually fits
/// (`matched_bracket_row_span`).
///
/// Builds a 12-line file with `{` on row 0 and `}` on row 5 (a 6-row
/// span), renders once with the cursor at the very end of the file
/// (row 11) to force the viewport to scroll away from row 0 first --
/// matching how a real file this doesn't naturally start on-screen at
/// once cursor moves around -- then moves the cursor onto the `}` and
/// renders again. `content_height` is deliberately chosen (`area`
/// height 8, minus the 2-row border) to be *exactly* the pair's own
/// span (6), so it fits precisely.
#[test]
fn a_multi_line_bracket_pair_widens_the_viewport_to_show_both_when_it_fits() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let content = "{\na\nb\nc\nd\n}\ne\nf\ng\nh\ni\nj\n";
    let (mut editor, _path) = open_test_editor(content);

    let theme = Theme::dark();
    let backend = TestBackend::new(20, 8); // content_height = 8 - 2 = 6
    let mut terminal = Terminal::new(backend).unwrap();

    // Warm-up render at the default cursor position (row 0) -- `edtui`
    // only caches the real content height in `EditorState` as part of
    // rendering, so the very first render of a freshly-opened editor
    // has no prior height to scroll-adjust against yet.
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    // Now render with the cursor at the very last line -- scrolls the
    // viewport away from row 0 before the bracket-matching fix ever
    // gets a chance to act.
    editor.state.cursor = Index2::new(11, 0);
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    let (_, offset_y_before) = editor.state.viewport_offset();
    assert!(offset_y_before > 0, "sanity check: the viewport should have scrolled away from row 0 to keep row 11 visible");

    // Now move the cursor onto the '}' at row 5 and render again.
    editor.state.cursor = Index2::new(5, 0);
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    let (_, offset_y_after) = editor.state.viewport_offset();
    assert_eq!(offset_y_after, 0, "the viewport should have widened to row 0 so the whole pair fits");

    // The cursor (on row 5) and row 0's own '{' share the same column
    // (both are the first character of their line), so row 0's screen
    // cell is at the cursor's own screen x, one row below the top
    // border (screen y = 1, since the viewport offset is now 0).
    let cursor_pos = editor.cursor_screen_position().expect("cursor should be visible");
    let buf = terminal.backend().buffer();
    let highlight_style = (theme.text, theme.border);
    let top_row_cell = &buf[(cursor_pos.x, 1)];
    assert_eq!((top_row_cell.fg, top_row_cell.bg), highlight_style, "the '{{' on row 0 should now be visible and highlighted");
}

/// The "doesn't fit" half of the same fix: when the pair's own span is
/// taller than the available content height, the viewport must *not*
/// be forced to include both -- keeping the cursor's own row visible
/// (`edtui`'s own default behavior) is the correct fallback, per
/// `matched_bracket_row_span`'s own doc comment.
#[test]
fn a_bracket_pair_that_does_not_fit_leaves_the_viewport_showing_the_cursor() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    // '{' on row 0, '}' on row 11 -- a 12-row span, taller than any
    // viewport this test uses.
    let content = "{\na\nb\nc\nd\ne\nf\ng\nh\ni\nj\n}\n";
    let (mut editor, _path) = open_test_editor(content);

    let theme = Theme::dark();
    let backend = TestBackend::new(20, 8); // content_height = 6, less than the 12-row span
    let mut terminal = Terminal::new(backend).unwrap();

    // Warm-up render at the default cursor position -- `edtui` only
    // caches the real content height as part of rendering, so the very
    // first render of a freshly-opened editor has no prior height to
    // scroll-adjust against yet.
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    editor.state.cursor = Index2::new(11, 0); // on the '}'
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    let cursor_pos = editor.cursor_screen_position().expect("cursor should still be visible");
    let buf = terminal.backend().buffer();
    let cursor_cell = &buf[(cursor_pos.x, cursor_pos.y)];
    assert_eq!(
        (cursor_cell.fg, cursor_cell.bg),
        (theme.text, theme.border),
        "the cursor's own '}}' should still render highlighted -- it's still a real match, just the far side doesn't fit on screen"
    );
}

/// Regression/documentation test for a real report, with a screenshot,
/// asking whether this was an `edtui` bug: repeatedly pressing `Right`
/// on a `.editorconfig` file's own first line ("root = true", Vim
/// mode) appeared to "stop" partway along the line. Reproduced with
/// the exact same content -- it isn't a bug, it's genuine Vim
/// behavior: `Right`/`l` in `Normal` mode is bound to `edtui`'s own
/// `MoveForward`, clamped by `max_col_normal` to `line.len() - 1` (the
/// line's own *last real character*, never one column past it the way
/// `Insert` mode's own append position allows) -- confirmed directly
/// from `edtui`'s source. "root = true" is 11 characters (indices
/// 0..=10), so column 10 (the final 'e') is correctly the last
/// reachable column; further presses are a no-op, and -- unlike this
/// project's own `Standard`-mode line-boundary wrapping
/// (`wrap_line_boundary_arrow_movement`, deliberately not active in
/// `Vim` at all) -- real Vim's own `l` never wraps onto the next line
/// either, so there's genuinely nowhere further right for the cursor
/// to go without a different key (`a`/`A`/`$`/entering `Insert`).
#[test]
fn vim_right_arrow_stops_at_the_last_character_of_a_line_not_a_bug() {
    let path = unique_scratch_dir("editor").join(".editorconfig");
    let content = "root = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\n";
    fs::write(&path, content).expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");

    for _ in 0..20 {
        editor.input(key(KeyCode::Right));
    }

    assert_eq!(
        editor.state.cursor,
        Index2::new(0, 10),
        "cursor should settle on column 10, the last real character ('e') of \"root = true\" (11 chars, indices 0..=10) -- \
         Normal mode's own max_col clamp, not a bug"
    );
}


