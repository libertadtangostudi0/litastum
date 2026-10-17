use ratatui::{backend::TestBackend, Terminal};

use super::*;
use crate::editor::EditorKeymapMode;
use crate::test_support::unique_scratch_dir;

fn open_pair(left_content: &str, right_content: &str) -> CompareState {
    let dir = unique_scratch_dir("ui-compare");
    let left_path = dir.join("left.txt");
    let right_path = dir.join("right.txt");
    std::fs::write(&left_path, left_content).unwrap();
    std::fs::write(&right_path, right_content).unwrap();
    CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap()
}

/// Compare panes have no syntax colors (they competed with the diff). Uses
/// `.rs`, which `syntect` recognizes -- `.txt` couldn't tell "disabled"
/// from "unrecognized".
#[test]
fn syntax_highlighting_is_disabled_even_for_a_recognized_language() {
    let dir = crate::test_support::unique_scratch_dir("ui-compare-no-syntax");
    let left_path = dir.join("left.rs");
    let right_path = dir.join("right.rs");
    let content = "fn main() {\n    // a comment\n    let x = 1;\n}\n";
    std::fs::write(&left_path, content).unwrap();
    std::fs::write(&right_path, content).unwrap();
    let mut state = CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap();
    let theme = Theme::dark();

    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

    // Every rendered cell in the left pane's content area (past the
    // border and line-number gutter) should be plain `theme.text` --
    // a real syntax highlighter would color "fn"/the comment/the
    // numeric literal distinctly from this.
    for y in 1..6u16 {
        for x in 4..28u16 {
            let cell = &buffer[(x, y)];
            if cell.symbol() != " " {
                assert_eq!(cell.fg, theme.text, "cell ({x},{y}) = {:?} should be plain theme.text, not syntax-colored", cell.symbol());
            }
        }
    }
}

fn render(state: &mut CompareState, theme: &Theme, line_ending_display: LineEndingDisplay) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(60, 11);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| { draw_compare(frame, frame.area(), state, theme, line_ending_display); }).unwrap();
    terminal.backend().buffer().clone()
}

/// A changed row should carry the diff background on both panes;
/// an unchanged row should carry neither. Checks across each row's
/// full x range rather than one hand-picked column, since a
/// `Highlight` only ever covers a line's own real character span --
/// not the empty space past it -- and the exact column that lands
/// on depends on the line-number gutter's own width.
#[test]
fn changed_lines_get_a_diff_colored_background() {
    let mut state = open_pair("aaaa\nbbbb\ncccc\n", "aaaa\nxxxx\ncccc\n");
    let theme = Theme::dark();
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

    let row_has_bg = |buffer: &ratatui::buffer::Buffer, x_range: std::ops::Range<u16>, y: u16, bg: ratatui::style::Color| x_range.clone().any(|x| buffer[(x, y)].bg == bg);

    // Row 0 ("aaaa", unchanged) is at screen y = 1 (past the
    // border); row 1 ("bbbb"/"xxxx", changed) is at y = 2.
    assert!(!row_has_bg(&buffer, 0..30, 1, theme.diff_removed_bg), "unchanged row 0 shouldn't carry a diff background");
    assert!(row_has_bg(&buffer, 0..30, 2, theme.diff_removed_bg), "left's changed row should be red somewhere");
    assert!(row_has_bg(&buffer, 30..60, 2, theme.diff_added_bg), "right's changed row should be green somewhere");
}

/// The unfocused pane really renders at the diff-mapped row after the
/// focused one scrolls -- checked on the rendered text, not just
/// `viewport_top_row()`.
#[test]
fn the_unfocused_panes_viewport_tracks_the_focused_one() {
    let left = "a\nb\nc\nd\ne\nf\ng\nh\n";
    let right = "a\nnew\nb\nc\nd\ne\nf\ng\nh\n";
    let mut state = open_pair(left, right);
    let theme = Theme::dark();

    // Scroll the focused (left) pane so real row 3 ("d") is at the
    // top of its viewport.
    state.left.set_viewport_top_row(3);
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

    let right_top_row_text: String = (30..60).map(|x| buffer[(x, 1)].symbol().to_string()).collect();
    assert!(right_top_row_text.contains('d'), "right's own top row should have followed left's scroll to \"d\", not stayed at \"a\" (row 0)");
}

/// Requested: the other pane lagged a key behind the caret. Moving the
/// caret past the screen's edge scrolls the focused pane while it's
/// drawn, so the other one must follow in the same frame.
#[test]
fn the_other_pane_follows_a_caret_scroll_in_the_same_frame() {
    let text: String = (0..40).map(|row| format!("line{row:02}\n")).collect();
    let mut state = open_pair(&text, &text);
    let theme = Theme::dark();
    let row_text = |buffer: &ratatui::buffer::Buffer, x: std::ops::Range<u16>| -> String { x.map(|x| buffer[(x, 1)].symbol().to_string()).collect() };

    for press in 0..15 {
        state.left.input(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Down));
        let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);
        let (left_top, right_top) = (row_text(&buffer, 0..30), row_text(&buffer, 30..60));
        let left_line = left_top.split_whitespace().find(|word| word.starts_with("line")).unwrap().to_string();
        assert!(right_top.contains(&left_line), "press {press}: left shows {left_line} at the top, right shows {right_top:?}");
    }
}

/// `F8` to a hunk below the screen centers it, like a `Ctrl+F` match.
/// An 11-row pane: 9 content
/// rows, so row 40 lands 4 rows down. History: docs/history/compare.md.
#[test]
fn a_hunk_jump_off_screen_is_centered() {
    let left: String = (0..60).map(|row| format!("line {row}\n")).collect();
    let right = left.replace("line 40\n", "changed\n");
    let mut state = open_pair(&left, &right);
    let theme = Theme::dark();
    render(&mut state, &theme, LineEndingDisplay::Hidden);

    state.jump_to_next_hunk();
    render(&mut state, &theme, LineEndingDisplay::Hidden);

    assert_eq!(state.left.cursor().row, 40);
    assert_eq!(state.left.viewport_top_row(), 36);
}

/// With long lines wrapped in the narrow panes, the hunk still lands in
/// the middle -- counted in screen rows, not buffer rows -- and both
/// panes show the same rows. Before, the wrapped rows above pushed it
/// below the middle and `edtui`'s own re-scroll left the other pane a
/// row off.
#[test]
fn a_hunk_jump_is_centered_in_screen_rows_when_lines_wrap() {
    // Every third line is 78 characters: 4 screen rows in a 25-column
    // text area (28 inside the border, minus a 3-column gutter).
    let left: String = (0..60).map(|row| format!("line {row} {}\n", "x".repeat(if row % 3 == 0 { 70 } else { 0 }))).collect();
    let right = left.replacen("line 40 ", "CHANGED ", 1);
    let mut state = open_pair(&left, &right);
    let theme = Theme::dark();
    render(&mut state, &theme, LineEndingDisplay::Hidden);

    state.jump_to_next_hunk();
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

    // Row 39 wraps to 4 screen rows, so the view starts there and the
    // hunk (row 40) is on screen row 5 of 9 -- y = 5 past the border.
    assert_eq!(state.left.viewport_top_row(), 39);
    let row_text = |x_range: std::ops::Range<u16>| x_range.map(|x| buffer[(x, 5)].symbol().to_string()).collect::<String>();
    assert!(row_text(0..30).contains("line 40"), "{}", row_text(0..30));
    assert!(row_text(30..60).contains("CHANGED"), "the other pane shows the same row: {}", row_text(30..60));
}

/// A hunk already on screen is centered too, not left where it was.
#[test]
fn a_visible_hunk_is_centered_as_well() {
    let left: String = (0..60).map(|row| format!("line {row}\n")).collect();
    let right = left.replace("line 20\n", "c20\n").replace("line 24\n", "c24\n");
    let mut state = open_pair(&left, &right);
    let theme = Theme::dark();
    render(&mut state, &theme, LineEndingDisplay::Hidden);
    state.jump_to_next_hunk();
    render(&mut state, &theme, LineEndingDisplay::Hidden);
    assert_eq!(state.left.viewport_top_row(), 16);

    state.jump_to_next_hunk();
    render(&mut state, &theme, LineEndingDisplay::Hidden);

    assert_eq!(state.left.cursor().row, 24, "row 24 was already on screen (rows 16-24)");
    assert_eq!(state.left.viewport_top_row(), 20);
}

fn left_click(column: u16, row: u16) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }
}

/// A click in the unfocused pane focuses it and puts the caret where
/// it was clicked. Panes are 30 columns wide with a 2-column gutter
/// ("1 ") inside the border, so text starts at x = 3 (left) and x = 33
/// (right); content row 2 is at y = 3.
#[test]
fn a_click_focuses_the_pane_and_places_the_caret() {
    let mut state = open_pair("aaaa\nbbbb\ncccc\n", "aaaa\nxxxx\ncccc\n");
    let theme = Theme::dark();
    render(&mut state, &theme, LineEndingDisplay::Hidden);
    assert_eq!(state.focus, crate::compare::Side::Left);

    state.mouse(left_click(35, 3));
    render(&mut state, &theme, LineEndingDisplay::Hidden);

    assert_eq!(state.focus, crate::compare::Side::Right);
    assert_eq!(state.right.cursor(), edtui::Index2::new(2, 2));

    state.mouse(left_click(4, 2));

    assert_eq!(state.focus, crate::compare::Side::Left);
    assert_eq!(state.left.cursor(), edtui::Index2::new(1, 1));
}

/// A click on the right pane's title opens its path field there,
/// focused, with the caret after the visible end of the path.
#[test]
fn a_click_on_a_title_edits_that_panes_path() {
    let mut state = open_pair("aaaa\n", "bbbb\n");
    // A bright selection color with dark text over it, like the scheme
    // in the report -- plain `theme.text` there was unreadable.
    let mut theme = Theme::dark();
    theme.selection_text = Some(Color::Rgb(0, 0, 0));
    render(&mut state, &theme, LineEndingDisplay::Hidden);

    state.mouse(left_click(40, 0));
    assert_eq!(state.focus, crate::compare::Side::Right);
    assert!(state.path_edit.is_some());

    let backend = TestBackend::new(60, 11);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut cursor = None;
    terminal.draw(|frame| cursor = draw_compare(frame, frame.area(), &mut state, &theme, LineEndingDisplay::Hidden)).unwrap();
    assert_eq!(cursor.map(|position| position.y), Some(0), "the caret is in the field on the border row");
    let caret = cursor.unwrap();
    assert_eq!(terminal.backend().buffer()[(caret.x, caret.y)].symbol(), " ", "the cell after the path is empty, not the title's leftover");
    let cell = &terminal.backend().buffer()[(31, 0)];
    assert_eq!((cell.fg, cell.bg), (theme.text, theme.bg), "unselected text is plain");

    // Reported: the whole field was filled and a selection cleared the
    // fill; the selection is what gets it.
    state.path_edit.as_mut().unwrap().field.select_all();
    terminal.draw(|frame| { draw_compare(frame, frame.area(), &mut state, &theme, LineEndingDisplay::Hidden); }).unwrap();
    let cell = &terminal.backend().buffer()[(31, 0)];
    assert_eq!((cell.fg, cell.bg), (Color::Rgb(0, 0, 0), theme.current_row_bg), "the scheme's selection text color, as on a selected panel row");
    assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));

    state.mouse(left_click(40, 3));
    assert!(state.path_edit.is_none(), "a click in the text puts the title back");
}

/// The diff and its highlights are kept between frames now; an edit
/// must still bring them up to date on the very next one.
#[test]
fn an_edit_updates_the_diff_highlight_on_the_next_frame() {
    let mut state = open_pair("aaaa\nbbbb\n", "aaaa\nbbbx\n");
    let theme = Theme::dark();
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);
    let row_has_bg = |buffer: &ratatui::buffer::Buffer, y: u16| (0..30).any(|x| buffer[(x, y)].bg == theme.diff_removed_bg);
    assert!(row_has_bg(&buffer, 2), "bbbb differs");

    state.left.set_cursor(edtui::Index2::new(1, 3));
    state.left.input(crate::test_support::key(crossterm::event::KeyCode::Delete));
    state.left.input(crate::test_support::key(crossterm::event::KeyCode::Char('x')));
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

    assert!(!row_has_bg(&buffer, 2), "bbbx now matches");
}

/// Reported with an Araxis screenshot: commenting a line out lit the
/// whole line. Only the `//` is in the strong color now; the rest of
/// the line is a fainter one.
#[test]
fn only_the_changed_characters_of_a_line_are_highlighted() {
    let mut state = open_pair("#define X\n", "//#define X\n");
    let theme = Theme::dark();
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);
    // Right pane: border at x = 30, a 2-column gutter, text from x = 33.
    let bg = |x: u16| buffer[(x, 1)].bg;
    let faint = ChangeColors::from(theme.success, theme.diff_added_bg, &theme).line;

    assert_eq!((bg(33), bg(34)), (theme.diff_added_bg, theme.diff_added_bg), "//");
    assert_eq!(bg(35), faint, "#define is unchanged");
    assert_eq!(buffer[(3, 1)].bg, ChangeColors::from(theme.danger, theme.diff_removed_bg, &theme).line, "the left line has nothing removed");
}

/// A line added as a whole, with nothing opposite, stays strong
/// throughout.
#[test]
fn an_added_line_is_highlighted_whole() {
    let mut state = open_pair("a\n", "a\nnew\n");
    let theme = Theme::dark();
    let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);
    assert!((33..36).all(|x| buffer[(x, 2)].bg == theme.diff_added_bg));
}

#[test]
fn line_ending_markers_show_up_only_when_shown() {
    let mut state = open_pair("a\r\n", "a\n");
    let theme = Theme::dark();

    let hidden = render(&mut state, &theme, LineEndingDisplay::Hidden);
    let hidden_text: String = (0..60).map(|x| hidden[(x, 1)].symbol().to_string()).collect();
    assert!(!hidden_text.contains("CRLF"), "no marker at all while Hidden");

    let shown = render(&mut state, &theme, LineEndingDisplay::Shown);
    let shown_text: String = (0..60).map(|x| shown[(x, 1)].symbol().to_string()).collect();
    assert!(shown_text.contains("CRLF"), "left's own CRLF-terminated line should show its marker while Shown");
}
