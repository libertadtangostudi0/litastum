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
