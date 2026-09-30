use super::*;

#[test]
fn ctrl_f_opens_the_search_box() {
    let (mut app, _path) = open_editor_app("hello world\n");

    handle_editor_key(&mut app, ctrl_key('f')).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.is_searching());
}

#[test]
fn f7_opens_the_search_box_like_ctrl_f() {
    let (mut app, _path) = open_editor_app("hello world\n");

    handle_editor_key(&mut app, key(KeyCode::F(7))).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.is_searching());
}

#[test]
fn typing_filters_the_query_live_and_jumps_to_the_first_match() {
    let (mut app, _path) = open_editor_app("hello world\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();

    for c in "world".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "world");
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 6 }, "cursor should jump to \"world\"'s own start");
}

#[test]
fn backspace_removes_the_last_query_character() {
    let (mut app, _path) = open_editor_app("hello world\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('w'))).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('o'))).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "w");
}

/// Real requirement, stated directly: navigation is plain `Up`/
/// `Down`, not `F3`/`Shift+F3` -- there's no bare-arrow conflict
/// to work around here the way the always-live command line has,
/// since this is its own popup.
#[test]
fn enter_and_shift_enter_navigate_between_matches() {
    let (mut app, _path) = open_editor_app("cat dog cat\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "cat".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 }, "sanity: should start on the first \"cat\"");

    handle_editor_key(&mut app, key(KeyCode::Enter)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 8 }, "Enter should jump to the second \"cat\"");

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 }, "Shift+Enter should jump back to the first \"cat\"");
}

/// Real requirement, stated directly after `Up`/`Down` was
/// first tried for match navigation and reported wrong: those
/// keys browse the *search history* instead, a shell-`Up`-arrow
/// convention -- first press recalls the most recent past
/// query, further presses step further back, `Down` steps back
/// toward the present and clears the box once past the newest
/// entry.
#[test]
fn up_and_down_browse_search_history_not_matches() {
    let (mut app, _path) = open_editor_app("cat dog cat\n");
    app.search_history = vec!["dog".to_string(), "cat".to_string()];
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "cat", "first Up should recall the most recent past query");

    handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "dog", "second Up should step further back");

    handle_editor_key(&mut app, key(KeyCode::Down)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "cat", "Down should step back toward the most recent entry");

    handle_editor_key(&mut app, key(KeyCode::Down)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "", "Down past the newest entry should clear the box");
}

#[test]
fn typing_after_browsing_history_resets_it() {
    let (mut app, _path) = open_editor_app("hello world\n");
    app.search_history = vec!["hello".to_string()];
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "hello", "sanity: history recalled");

    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "hello!");

    // A further Up should start fresh from the most recent
    // entry again, not continue on from wherever browsing left
    // off before the edit.
    handle_editor_key(&mut app, key(KeyCode::Up)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "hello");
}

#[test]
fn esc_leaves_the_cursor_right_after_the_found_match() {
    let (mut app, _path) = open_editor_app("hello world\n");
    let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
    editor.input(key(KeyCode::Right));
    editor.input(key(KeyCode::Right)); // cursor now at column 2, before opening search
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "world".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(!editor.is_searching(), "should have closed the box");
    assert_eq!(
        editor.cursor().col,
        11,
        "should land right after \"world\"'s own last letter ('d', column 10) -- not on it, and not revert to where search started"
    );
}

/// Regression test for the real report: searching "lso" inside
/// "also" left the cursor visually *between* 's' and the final
/// 'o' instead of after it -- `stop_search` was landing directly
/// *on* the match's own last character, which only reads
/// correctly while a selection is active (`cursor_screen_position`'s
/// own +1 rendering shift, which doesn't fire here since closing
/// the search box never sets `state.selection`).
#[test]
fn esc_lands_after_the_match_not_visually_one_short_of_it() {
    let (mut app, _path) = open_editor_app("also\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "lso".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor().col, 4, "should be right after the final 'o' (column 3), not on it");
}

/// The revert-to-where-search-started behavior still applies
/// when nothing was actually found -- there's no match to leave
/// the cursor on.
#[test]
fn esc_with_no_match_found_reverts_to_where_search_started() {
    let (mut app, _path) = open_editor_app("hello world\n");
    let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
    editor.input(key(KeyCode::Right));
    editor.input(key(KeyCode::Right)); // cursor now at column 2
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "xyz".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor().col, 2, "nothing was found -- should revert to where search started");
}

/// Real requirement, stated directly: a separate search-history
/// file, recorded the same way `command_line::history` is.
#[test]
fn esc_records_a_non_empty_query_into_search_history() {
    let (mut app, _path) = open_editor_app("hello world\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "world".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert_eq!(app.search_history, vec!["world"]);
}

#[test]
fn esc_with_an_empty_query_records_nothing() {
    let (mut app, _path) = open_editor_app("hello world\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(app.search_history.is_empty());
}

/// Real requirement, stated directly: the query field should
/// offer history-based suggestions "similar to the command
/// line" -- `End` accepts the ghost-text suggestion shown after
/// the typed query (`find_history::suggest`, rendered by
/// `ui::editor_find::draw_find_popup`).
#[test]
fn end_accepts_the_history_suggestion() {
    let (mut app, _path) = open_editor_app("hello world\n");
    app.search_history = vec!["world".to_string()];
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('w'))).unwrap();

    handle_editor_key(&mut app, key(KeyCode::End)).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "world");
}

/// Requested directly: the box should edit like Find file's fields
/// -- `Shift+Left` selects, and typing replaces the selection, with
/// the matches following the new query immediately.
#[test]
fn typing_over_a_shift_selection_replaces_it_and_re_searches() {
    let (mut app, _path) = open_editor_app("cat cot\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "cat".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }
    handle_editor_key(&mut app, key(KeyCode::Left)).unwrap(); // cursor between 'a' and 't'
    handle_editor_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap(); // selects 'a'

    handle_editor_key(&mut app, key(KeyCode::Char('o'))).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "cot");
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 4 }, "should have jumped to \"cot\"");
}

#[test]
fn ctrl_shift_left_selects_a_whole_word_of_the_query() {
    let (mut app, _path) = open_editor_app("hello world\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "hello world".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "hello ", "Backspace should remove the selected word, not one character");
}

#[test]
fn typing_after_moving_the_cursor_left_inserts_mid_query() {
    let (mut app, _path) = open_editor_app("xx worlds\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "wrld".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }
    handle_editor_key(&mut app, key(KeyCode::Home)).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Right)).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Char('o'))).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "world");
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 3 }, "the fixed query should match \"world\" inside \"worlds\"");
}

/// Reported directly: `Ctrl+X` did nothing to a selection in the box.
/// The clipboard itself is inert in a test build
/// (`text_field::os_clipboard`), so this checks the cut's own effect
/// on the query and the matches.
#[test]
fn ctrl_x_cuts_the_selection_and_re_searches() {
    let (mut app, _path) = open_editor_app("cat catalog\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    for c in "catalog".chars() {
        handle_editor_key(&mut app, key(KeyCode::Char(c))).unwrap();
    }
    for _ in 0..4 {
        handle_editor_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)).unwrap(); // selects "alog"
    }

    handle_editor_key(&mut app, ctrl_key('x')).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "cat");
    assert_eq!(editor.search_field().unwrap().selection(), None, "nothing left selected after a cut");
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 }, "the shorter query matches the first \"cat\"");
}

#[test]
fn ctrl_x_with_nothing_selected_changes_nothing() {
    let (mut app, _path) = open_editor_app("hello\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('h'))).unwrap();

    handle_editor_key(&mut app, ctrl_key('x')).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "h");
}

/// `End` still accepts the history suggestion, but only from the end
/// of the query -- from the middle it first moves there, like `End`
/// in any other text field.
#[test]
fn end_mid_query_moves_to_the_end_before_accepting_a_suggestion() {
    let (mut app, _path) = open_editor_app("hello world\n");
    app.search_history = vec!["world".to_string()];
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('w'))).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('o'))).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Left)).unwrap();

    handle_editor_key(&mut app, key(KeyCode::End)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "wo", "first End only moves the cursor");
    assert_eq!(editor.search_field().unwrap().cursor(), 2);

    handle_editor_key(&mut app, key(KeyCode::End)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "world", "second End accepts the suggestion");
}

/// Draws the editor once, the way `ui::draw` would -- `edtui` maps a
/// mouse position through the screen area it recorded while drawing.
fn render(app: &mut App) {
    let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
    let theme = crate::theming::Theme::dark();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 8)).unwrap();
    terminal.draw(|frame| frame.render_widget(editor.view(&theme, frame.area()), frame.area())).unwrap();
}

/// Clicks the cell holding buffer column `col` on the caret's own row
/// -- found relative to where the caret itself renders, so the
/// border/line-number gutter widths never have to be hardcoded here.
fn click_same_row_at_col(app: &mut App, col: u16) {
    render(app);
    let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
    let caret_col = editor.cursor().col as u16;
    let caret_screen = editor.cursor_screen_position().expect("caret should be on screen");
    let mouse = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: caret_screen.x - caret_col + col,
        row: caret_screen.y,
        modifiers: KeyModifiers::NONE,
    };
    editor.mouse(mouse);
}

fn search_for(app: &mut App, query: &str) {
    handle_editor_key(app, ctrl_key('f')).unwrap();
    for c in query.chars() {
        handle_editor_key(app, key(KeyCode::Char(c))).unwrap();
    }
}

/// Requested directly, matching VS Code: a click in the text moves
/// the caret and keyboard focus there, while the box stays open with
/// its match still highlighted -- and the arrows then move the caret,
/// not the box's own cursor.
#[test]
fn a_click_in_the_text_moves_focus_there_but_keeps_the_box_and_its_match() {
    let (mut app, _path) = open_editor_app("hello world\n");
    search_for(&mut app, "world");

    click_same_row_at_col(&mut app, 2);

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(!editor.is_searching(), "keyboard focus moved to the text");
    assert!(editor.search_box_open(), "the box itself stays open");
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 2 });

    handle_editor_key(&mut app, key(KeyCode::Right)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 3 }, "arrows now move the caret");
    assert_eq!(editor.search_query(), "world", "the query is untouched");
}

/// With the text focused, typing edits the text -- and the open box's
/// match follows the edit instead of pointing at a stale position.
#[test]
fn editing_the_text_with_the_box_open_keeps_its_match_in_step() {
    let (mut app, _path) = open_editor_app("hello world\n");
    search_for(&mut app, "world");
    click_same_row_at_col(&mut app, 0);

    handle_editor_key(&mut app, key(KeyCode::Char('X'))).unwrap();

    let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
    assert!(editor.is_dirty(), "the keystroke went to the text, not the box");
    editor.search_next();
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 7 }, "\"world\" moved one column right, and the match with it");
}

#[test]
fn f3_and_shift_f3_step_through_matches_from_the_caret_while_the_text_has_focus() {
    let (mut app, _path) = open_editor_app("cat dog cat dog cat\n");
    search_for(&mut app, "cat");
    click_same_row_at_col(&mut app, 5);

    handle_editor_key(&mut app, key(KeyCode::F(3))).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 8 }, "the next match after the caret, not after the last selected one");

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 });
}

/// Far's `Shift+F7`/`Alt+F7` do what `F3`/`Shift+F3` do.
#[test]
fn shift_f7_and_alt_f7_step_through_matches_like_f3() {
    let (mut app, _path) = open_editor_app("cat dog cat dog cat\n");
    search_for(&mut app, "cat");
    click_same_row_at_col(&mut app, 5);

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(7), KeyModifiers::SHIFT)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 8 });

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 0 });
}

/// `Esc` with the text focused closes just the box -- not the
/// editor -- and leaves the caret where it is.
#[test]
fn esc_with_the_text_focused_closes_only_the_box() {
    let (mut app, _path) = open_editor_app("hello world\n");
    search_for(&mut app, "world");
    click_same_row_at_col(&mut app, 2);

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("the editor must stay open") };
    assert!(!editor.search_box_open());
    assert_eq!(editor.cursor(), edtui::Index2 { row: 0, col: 2 });
}

/// `Ctrl+F` while the text has focus gives it back to the box with
/// the query selected, VS Code-style -- typing replaces it.
#[test]
fn ctrl_f_with_the_text_focused_refocuses_the_box_with_the_query_selected() {
    let (mut app, _path) = open_editor_app("hello world\n");
    search_for(&mut app, "world");
    click_same_row_at_col(&mut app, 0);

    handle_editor_key(&mut app, ctrl_key('f')).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.is_searching());
    assert_eq!(editor.search_field().unwrap().selection(), Some((0, 5)), "the whole query is selected");

    handle_editor_key(&mut app, key(KeyCode::Char('h'))).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert_eq!(editor.search_query(), "h", "typing replaced the selected query");
}

#[test]
fn plain_keys_are_swallowed_by_the_search_box_not_forwarded_to_the_buffer() {
    let (mut app, _path) = open_editor_app("hello world\n");
    handle_editor_key(&mut app, ctrl_key('f')).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap();

    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(!editor.is_dirty(), "typing into the search box must not edit the buffer");
}
