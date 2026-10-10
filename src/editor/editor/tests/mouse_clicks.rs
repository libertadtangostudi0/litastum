use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;


/// Draws `editor` into a 40x6 screen and returns the buffer.
fn draw(editor: &mut Editor) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&Theme::dark(), frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    terminal.backend().buffer().clone()
}


/// Where `wanted` is drawn on screen row `y`.
fn column_of(buffer: &ratatui::buffer::Buffer, y: u16, wanted: &str) -> u16 {
    (0..buffer.area.width).find(|&x| buffer[(x, y)].symbol() == wanted).unwrap_or_else(|| panic!("{wanted:?} not drawn on row {y}"))
}


fn click(column: u16, row: u16) -> MouseEvent {
    MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
}


fn release(column: u16, row: u16) -> MouseEvent {
    MouseEvent { kind: MouseEventKind::Up(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
}


/// Reported: a click put the caret one character left of where it
/// landed. A click on a character puts the caret before that character.
#[test]
fn a_click_puts_the_caret_on_the_clicked_character() {
    let (mut editor, _path) = open_test_editor("first\nabcdef\n");
    let buffer = draw(&mut editor);
    let y = (0..buffer.area.height).find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "d")).unwrap();
    let x = column_of(&buffer, y, "d");

    editor.mouse(click(x, y));
    editor.mouse(release(x, y));

    assert_eq!(editor.cursor(), Index2::new(1, 3), "on \"d\"");
    draw(&mut editor);
    assert_eq!(editor.cursor_screen_position().map(|position| position.x), Some(x), "the caret is drawn where the click was");
}


/// A click past a line's end puts the caret at its end.
#[test]
fn a_click_past_the_end_of_a_line_puts_the_caret_at_its_end() {
    let (mut editor, _path) = open_test_editor("first\nabc\n");
    let buffer = draw(&mut editor);
    let y = (0..buffer.area.height).find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "c")).unwrap();
    let x = column_of(&buffer, y, "c");

    editor.mouse(click(x + 5, y));
    editor.mouse(release(x + 5, y));

    assert_eq!(editor.cursor(), Index2::new(1, 3));
}


/// A click on a line's last character still puts the caret before it.
#[test]
fn a_click_on_the_last_character_puts_the_caret_before_it() {
    let (mut editor, _path) = open_test_editor("first\n    abc\n");
    let buffer = draw(&mut editor);
    let y = (0..buffer.area.height).find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "c")).unwrap();
    let x = column_of(&buffer, y, "c");

    editor.mouse(click(x, y));
    editor.mouse(release(x, y));

    assert_eq!(editor.cursor(), Index2::new(1, 6));
}


/// A drag still selects -- the correction is for a plain click only.
#[test]
fn a_drag_past_the_end_still_selects_the_line() {
    let (mut editor, _path) = open_test_editor("first\nabc\n");
    let buffer = draw(&mut editor);
    let y = (0..buffer.area.height).find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "a")).unwrap();
    let x = column_of(&buffer, y, "a");

    editor.mouse(click(x, y));
    editor.mouse(MouseEvent { kind: MouseEventKind::Drag(MouseButton::Left), column: x + 10, row: y, modifiers: KeyModifiers::NONE });
    editor.mouse(release(x + 10, y));

    let selection = editor.state.selection.clone().expect("the drag selects");
    assert_eq!((selection.start, selection.end), (Index2::new(1, 0), Index2::new(1, 2)), "all of \"abc\", inclusive");
}


/// A one-character line: a click on it keeps the caret before it, a click
/// right of it puts the caret after it.
#[test]
fn a_one_character_line_tells_a_click_on_it_from_one_past_it() {
    let (mut editor, _path) = open_test_editor("first
}
");
    let buffer = draw(&mut editor);
    let y = (0..buffer.area.height).find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "}")).unwrap();
    let x = column_of(&buffer, y, "}");

    editor.mouse(click(x, y));
    editor.mouse(release(x, y));
    assert_eq!(editor.cursor(), Index2::new(1, 0), "on it");

    editor.mouse(click(x + 1, y));
    editor.mouse(release(x + 1, y));
    assert_eq!(editor.cursor(), Index2::new(1, 1), "past it");
}


/// Our layout matches what `edtui` draws: a click on every drawn
/// character -- after tabs, wide characters, on wrapped rows, in a
/// scrolled view -- lands on that very character.
#[test]
fn every_drawn_character_is_where_a_click_on_it_lands() {
    let long: String = "abcdefghijklmnopqrstuvwxyz".repeat(3);
    let text = format!("a\tb\tc\n\u{4e2d}\u{6587}x\n{long}\n\nlast a\n");
    let (mut editor, _path) = open_test_editor(&text);
    let lines: Vec<Vec<char>> = text.lines().map(|line| line.chars().collect()).collect();

    for top in [0, 2] {
        editor.set_viewport_top_row(top);
        let buffer = draw(&mut editor);
        let text_left = editor.text_left();
        let mut checked = 0;
        for y in 1..buffer.area.height - 1 {
            for x in text_left..buffer.area.width - 1 {
                let symbol = buffer[(x, y)].symbol().to_string();
                if symbol.trim().is_empty() {
                    continue;
                }
                editor.mouse(click(x, y));
                editor.mouse(release(x, y));
                let cursor = editor.cursor();
                let under = lines[cursor.row].get(cursor.col).map(|ch| ch.to_string());
                assert_eq!(under.as_deref(), Some(symbol.as_str()), "top {top}: the click at ({x}, {y}) landed on {cursor:?}");
                checked += 1;
            }
        }
        assert!(checked > 30, "top {top}: only {checked} characters drawn");
    }
}


/// Draws `editor` into a `width` x 6 screen.
fn draw_wide(editor: &mut Editor, width: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, 6)).unwrap();
    terminal
        .draw(|frame| {
            let view = editor.view(&Theme::dark(), frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
    terminal.backend().buffer().clone()
}


/// A click -- press and release -- at `(x, y)`; the caret after it.
fn click_at(editor: &mut Editor, x: u16, y: u16) -> Index2 {
    editor.mouse(click(x, y));
    editor.mouse(release(x, y));
    editor.cursor()
}


/// Requested: every character of the English layout. A click on a
/// character's cell puts the caret before it; a click on its right half
/// -- which litastum's window reports as the next cell
/// (`gui/src/mouse.rs::column_at`) -- after it. The last one's right half
/// is past the line's end.
#[test]
fn every_printable_ascii_character_takes_a_click_before_and_after_it() {
    let table: String = ('!'..='~').collect();
    let (mut editor, _path) = open_test_editor(&format!("{table}\n"));
    let buffer = draw_wide(&mut editor, 120);
    let y = 1;

    let text_left = editor.text_left();
    for (index, ch) in table.chars().enumerate() {
        let x = (text_left..buffer.area.width).find(|&x| buffer[(x, y)].symbol() == ch.to_string()).unwrap();
        assert_eq!(click_at(&mut editor, x, y), Index2::new(0, index), "{ch:?}: on it");
        assert_eq!(click_at(&mut editor, x + 1, y), Index2::new(0, index + 1), "{ch:?}: on its right half");
    }
}


/// The same table wrapped over several rows: each row's characters,
/// and the right half of a row's last one stays on that row.
#[test]
fn the_ascii_table_wrapped_over_rows_takes_clicks_the_same_way() {
    let table: String = ('!'..='~').collect();
    let (mut editor, _path) = open_test_editor(&format!("{table}\n"));
    let width: u16 = 40;
    let buffer = draw_wide(&mut editor, width);
    let text_left = editor.text_left();
    let row_width = usize::from(width - 1 - text_left);

    for (index, ch) in table.chars().enumerate() {
        let (y, x) = (1 + (index / row_width) as u16, text_left + (index % row_width) as u16);
        if y >= 5 {
            break;
        }
        assert_eq!(buffer[(x, y)].symbol(), ch.to_string(), "{ch:?} is drawn at ({x}, {y})");
        assert_eq!(click_at(&mut editor, x, y), Index2::new(0, index), "{ch:?}: on it");
        let last_on_its_row = index % row_width == row_width - 1;
        let right_half = if last_on_its_row { index } else { index + 1 };
        assert_eq!(click_at(&mut editor, x + 1, y).col, right_half, "{ch:?}: on its right half");
    }
}


/// Spaces are characters too: a click lands between them as on letters.
#[test]
fn spaces_take_clicks_like_letters() {
    let (mut editor, _path) = open_test_editor("a b  c\n");
    let buffer = draw_wide(&mut editor, 40);
    let x = column_of(&buffer, 1, "a");

    for col in 0..6 {
        assert_eq!(click_at(&mut editor, x + col as u16, 1), Index2::new(0, col), "cell {col}");
    }
    assert_eq!(click_at(&mut editor, x + 6, 1), Index2::new(0, 6), "past the end");
}


/// Characters two cells wide -- CJK, fullwidth Latin, a tab: the first
/// cell (and its right half, the glyph's middle) before it... the second
/// cell, the glyph's right half, after it.
#[test]
fn two_cell_characters_take_a_click_on_each_half() {
    let line = "a\u{4e2d}b\u{ff41}c\td\u{6587}";
    let (mut editor, _path) = open_test_editor(&format!("{line}\n"));
    let buffer = draw_wide(&mut editor, 60);
    let x_a = column_of(&buffer, 1, "a");

    let mut x = x_a;
    for (index, ch) in line.chars().enumerate() {
        let cells = if ch == '\t' { 2 } else { unicode_width::UnicodeWidthChar::width(ch).unwrap() as u16 };
        if ch != '\t' {
            assert_eq!(buffer[(x, 1)].symbol(), ch.to_string(), "{ch:?} is drawn at {x}");
        }
        assert_eq!(click_at(&mut editor, x, 1), Index2::new(0, index), "{ch:?}: its first cell");
        if cells == 2 {
            assert_eq!(click_at(&mut editor, x + 1, 1), Index2::new(0, index + 1), "{ch:?}: its second cell");
        }
        assert_eq!(click_at(&mut editor, x + cells, 1), Index2::new(0, index + 1), "{ch:?}: right of it");
        x += cells;
    }
}
