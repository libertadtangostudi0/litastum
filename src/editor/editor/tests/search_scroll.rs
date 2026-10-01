use ratatui::backend::TestBackend;
use ratatui::Terminal;

use super::*;

/// 100 numbered lines, with `target` put on the given rows.
fn numbered_lines_with_targets(target_rows: &[usize]) -> String {
    (0..100).map(|row| if target_rows.contains(&row) { format!("target {row}\n") } else { format!("line {row}\n") }).collect()
}


/// Renders `editor` into a 20x12 area: 10 content rows inside the border.
fn render(editor: &mut Editor, terminal: &mut Terminal<TestBackend>) {
    let theme = Theme::dark();
    terminal
        .draw(|frame| {
            let view = editor.view(&theme, frame.area());
            frame.render_widget(view, frame.area());
        })
        .unwrap();
}


fn search(editor: &mut Editor, query: &str) {
    editor.start_search();
    for c in query.chars() {
        editor.search_push_char(c);
    }
}


/// A match below the screen lands in the middle of the view, not on its
/// last row. History: docs/history/editor-rendering.md.
#[test]
fn an_off_screen_match_is_centered() {
    let (mut editor, _path) = open_test_editor(&numbered_lines_with_targets(&[60]));
    let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
    render(&mut editor, &mut terminal);

    search(&mut editor, "target");
    render(&mut editor, &mut terminal);

    assert_eq!(editor.cursor_row(), 60);
    assert_eq!(editor.viewport_top_row(), 55, "row 60 sits 5 rows into a 10-row view");
}


/// A match near the top can't be centered: the view stays at the start.
#[test]
fn a_match_near_the_start_keeps_the_view_at_the_first_line() {
    let (mut editor, _path) = open_test_editor(&numbered_lines_with_targets(&[3]));
    let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
    render(&mut editor, &mut terminal);

    search(&mut editor, "target");
    render(&mut editor, &mut terminal);

    assert_eq!(editor.cursor_row(), 3);
    assert_eq!(editor.viewport_top_row(), 0);
}


/// A match already on screen is centered too.
#[test]
fn a_visible_match_lower_down_is_centered() {
    let (mut editor, _path) = open_test_editor(&numbered_lines_with_targets(&[8]));
    let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
    render(&mut editor, &mut terminal);

    search(&mut editor, "target");
    render(&mut editor, &mut terminal);

    assert_eq!(editor.cursor_row(), 8, "on screen before the jump (rows 0-9)");
    assert_eq!(editor.viewport_top_row(), 3);
}


#[test]
fn next_match_off_screen_is_centered_too() {
    let (mut editor, _path) = open_test_editor(&numbered_lines_with_targets(&[5, 70]));
    let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
    render(&mut editor, &mut terminal);
    search(&mut editor, "target");
    render(&mut editor, &mut terminal);
    assert_eq!(editor.viewport_top_row(), 0, "the first match is already visible");

    editor.search_next();
    render(&mut editor, &mut terminal);

    assert_eq!(editor.cursor_row(), 70);
    assert_eq!(editor.viewport_top_row(), 65);
}


#[test]
fn a_match_near_the_end_stops_at_the_last_line() {
    let (mut editor, _path) = open_test_editor(&numbered_lines_with_targets(&[98]));
    let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
    render(&mut editor, &mut terminal);

    search(&mut editor, "target");
    render(&mut editor, &mut terminal);

    let top = editor.viewport_top_row();
    assert_eq!(editor.cursor_row(), 98);
    assert!((top..top + 10).contains(&98), "the match is visible (top {top})");
    assert!(top + 10 >= 100, "no empty rows are scrolled into view past the end (top {top})");
}
