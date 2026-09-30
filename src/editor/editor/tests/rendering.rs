use super::*;


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


/// Same bug as the test above, hit again for `Ctrl+F` search: the
/// current match is painted in `selection_style` too, but a search
/// jump (typing, `Enter`, ...) puts `state.cursor` on the match's
/// *first* character, not its last (the
/// opposite convention from this app's own selection, which always
/// keeps the cursor on the trailing edge). With no exception for
/// `is_searching()`, that first cell fell into the same "no selection,
/// no matched bracket" branch as plain typing and got reset to `base`
/// -- reported directly with two screenshots: a match's own first
/// character never looked highlighted, and the correctly-highlighted
/// rest of the match right after the cursor read as a separate block
/// because of it.
#[test]
fn search_match_first_cell_renders_with_selection_color_not_base() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut editor, _path) = open_test_editor("hay needle hay");
    editor.start_search();
    for c in "needle".chars() {
        editor.search_push_char(c);
    }
    assert!(editor.is_searching());

    let theme = Theme::dark();
    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();

    // Search never shifts `cursor_screen_position()` the way an active
    // selection does (that shift only reads `self.state.selection`,
    // untouched during a search) -- the reported position already
    // points straight at the match's own first character.
    let cursor_pos = editor
        .cursor_screen_position()
        .expect("cursor should be visible after rendering");
    let buf = terminal.backend().buffer();
    let cell_bg = buf[(cursor_pos.x, cursor_pos.y)].bg;

    assert_eq!(
        cell_bg, theme.current_row_bg,
        "a search match's own first character -- where the cursor sits during search -- must \
         render with the selection color, not be reset to the base background by edtui's \
         cursor-cell paint"
    );
}


/// Same bug a third time, for `Compare`'s own diff row backgrounds
/// (`ui/compare.rs::row_highlights`, applied via
/// `Editor::set_extra_highlights`): reported directly with a
/// screenshot -- the cursor's own cell on an otherwise fully
/// diff-colored line rendered as a visibly different, plain patch cut
/// out of the middle of it, since neither selection, search, nor
/// bracket-matching covered this case and the cell fell through to
/// plain `base`.
#[test]
fn cursor_cell_on_a_diff_highlighted_row_keeps_the_diff_color_not_base() {
    use edtui::Highlight;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Style};
    use ratatui::Terminal;

    let (mut editor, _path) = open_test_editor("unchanged\nremoved line\n");
    editor.set_cursor(Index2::new(1, 0));
    let diff_style = Style::default().fg(Color::White).bg(Color::Red);
    editor.set_extra_highlights(vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 11), diff_style)]);

    let theme = Theme::dark();
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
    let cell_bg = buf[(cursor_pos.x, cursor_pos.y)].bg;

    assert_eq!(
        cell_bg,
        diff_style.bg.unwrap(),
        "the cursor's own cell on a diff-highlighted row must keep that row's diff color, not be \
         reset to the base background by edtui's cursor-cell paint"
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
