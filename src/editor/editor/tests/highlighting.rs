use super::*;


/// A file with one enormous line gets no syntax highlighting
/// (`has_pathologically_long_line`) -- `syntect` re-tokenized the whole
/// line on every redraw. No cell may differ from the base text color.
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


/// With the cursor on an opening bracket, the closing one renders in the
/// word-highlight style; a character between them doesn't. The cursor's
/// own cell is tested separately.
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


/// The near bracket (under the cursor) must change color too: `edtui`
/// paints the cursor cell last, so `Editor::view` paints it with the
/// highlight style when `cursor_is_on_a_matched_bracket`. History: docs/history/editor-rendering.md.
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


/// Bracket matching and word highlighting stay independent: on `(` only
/// `)` highlights, not the repeated word; on the word, only its other
/// occurrence.
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


/// The far bracket of a multi-line pair is scrolled into view when the
/// pair fits (`matched_bracket_row_span`). A 12-line file, `{` on row 0
/// and `}` on row 5: render with the cursor at the end first to scroll
/// away, then move onto `}`. The content height (8 - 2 border rows) is
/// exactly the pair's span. History: docs/history/editor-rendering.md.
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


/// Draws `editor` into a 20x8 screen (6 content rows).
fn draw_small(editor: &mut Editor, terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) {
    let theme = Theme::dark();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
}


/// Reported: the caret by a brace moved the text up -- the pair's top
/// row was put at the top of the view even when the whole pair was on
/// screen already.
#[test]
fn a_bracket_pair_already_on_screen_leaves_the_view_alone() {
    let content: String = (0..30).map(|row| match row {
        10 => "{\n".to_string(),
        12 => "}\n".to_string(),
        _ => format!("line {row}\n"),
    }).collect();
    let (mut editor, _path) = open_test_editor(&content);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 8)).unwrap();
    draw_small(&mut editor, &mut terminal);
    editor.set_viewport_top_row(8);
    draw_small(&mut editor, &mut terminal);

    editor.state.cursor = Index2::new(10, 0);
    draw_small(&mut editor, &mut terminal);

    assert_eq!(editor.state.viewport_offset().1, 8, "rows 8-13 show the whole pair already");
}


/// A pair partly off screen scrolls only as far as it needs.
#[test]
fn a_bracket_pair_partly_off_screen_scrolls_only_as_far_as_needed() {
    let content: String = (0..30).map(|row| match row {
        4 => "{\n".to_string(),
        8 => "}\n".to_string(),
        _ => format!("line {row}\n"),
    }).collect();
    let (mut editor, _path) = open_test_editor(&content);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 8)).unwrap();
    draw_small(&mut editor, &mut terminal);

    editor.state.cursor = Index2::new(4, 0);
    draw_small(&mut editor, &mut terminal);

    assert_eq!(editor.state.viewport_offset().1, 3, "rows 3-8: the '}}' on the last row, not the '{{' on the first");
}


/// Reported: a click by a brace moved the text. A click puts the caret
/// where it's seen; the view stays, the far brace off screen or not.
#[test]
fn a_click_by_a_brace_leaves_the_view_alone() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let content: String = (0..30).map(|row| match row {
        4 => "{\n".to_string(),
        8 => "}\n".to_string(),
        _ => format!("line {row}\n"),
    }).collect();
    let (mut editor, _path) = open_test_editor(&content);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 8)).unwrap();
    draw_small(&mut editor, &mut terminal);

    let (column, row) = (editor.text_left(), 1 + 4);
    for kind in [MouseEventKind::Down(MouseButton::Left), MouseEventKind::Up(MouseButton::Left)] {
        editor.mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE });
    }
    draw_small(&mut editor, &mut terminal);

    assert_eq!(editor.cursor(), Index2::new(4, 0), "on the '{{'");
    assert_eq!(editor.state.viewport_offset().1, 0, "the view stays");
}
