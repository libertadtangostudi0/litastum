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
    let editor = Editor::open(path.clone(), None).expect("open test fixture file");
    (editor, path)
}

/// Same as `open_test_editor`, but under a `.rs` name -- for tests that
/// need real syntax highlighting to actually be active (`file.txt`
/// above resolves to Plain Text, which colors nothing).
fn open_test_rust_editor(contents: &str) -> Editor {
    let path = unique_scratch_dir("editor").join("file.rs");
    fs::write(&path, contents).expect("write test fixture file");
    Editor::open(path, None).expect("open test fixture file")
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
            let view = editor.view(&theme);
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
            let view = editor.view(&theme);
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
            let view = editor.view(&theme);
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
