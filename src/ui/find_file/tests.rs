use std::path::PathBuf;

use ratatui::{backend::TestBackend, Terminal};

use super::*;

fn state_with(count: usize, selected: usize) -> FindFileState {
    FindFileState {
        phase: FindFilePhase::Results,
        query: "x".to_string(),
        cursor: 0,
        selection_anchor: None,
        content_query: String::new(),
        content_cursor: 0,
        content_selection_anchor: None,
        active_field: FindFileField::Name,
        name_history_index: None,
        content_history_index: None,
        results: (0..count).map(|i| PathBuf::from(format!("C:/dev/file_{i:04}.txt"))).collect(),
        selected,
        marked: Default::default(),
        pending: None,
        search_duration: Some(std::time::Duration::from_millis(5)),
        results_capped: false,
        export_message: None,
    }
}

fn rendered(state: &FindFileState) -> String {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Regression test for the real report: a result count far past
/// what the (terminal-height-clamped) popup can show at once used
/// to render every item into a fixed-height list with no scroll
/// offset at all -- the selected row, deep into a list of
/// thousands, was simply invisible, scrolled off past the bottom of
/// the rendered area with nothing to bring it into view. A real
/// `ListState` (see the call site's own doc comment) should keep
/// whichever row is selected actually on screen no matter how far
/// into a long list it is.
#[test]
fn selecting_a_result_far_down_a_long_list_scrolls_it_into_view() {
    let state = state_with(2000, 1500);

    let text = rendered(&state);

    assert!(text.contains("file_1500"), "the selected result should be scrolled into view:\n{text}");
}

#[test]
fn a_short_result_list_needs_no_scrolling_to_show_the_selection() {
    let state = state_with(3, 2);

    let text = rendered(&state);

    assert!(text.contains("file_0002"));
}

/// `Rounded` gets a separator under the title (matching
/// `ui/theme_menu.rs`'s own color-scheme picker) and another right
/// before the footer hints (matching `ui/confirm.rs`'s delete
/// popup) -- requested directly, alongside a screenshot of the
/// color-scheme picker's own title-plus-line look.
/// Requested directly, alongside the color-scheme picker's own
/// title-plus-line screenshot: the title should also get a bold,
/// un-padded look and a separator right under it in the Results
/// phase, same as `Typing` already has.
#[test]
fn rounded_style_results_show_a_separator_under_the_title() {
    let state = state_with(2, 0);
    let text = rendered(&state);
    let title_line_index = text.lines().position(|line| line.contains("Find file")).expect("title should render");
    let line_below = text.lines().nth(title_line_index + 1).unwrap();
    assert!(line_below.contains('─'), "a separator should sit right below the title: {line_below:?}");
}

#[test]
fn rounded_style_results_show_a_separator_before_the_hints() {
    let state = state_with(2, 0);
    let text = rendered(&state);
    let hint_line_index = text.lines().position(|line| line.contains("go to")).expect("hint row should render");
    let line_above = text.lines().nth(hint_line_index - 1).unwrap();
    assert!(line_above.contains('─'), "a separator should sit right above the footer hints: {line_above:?}");
}

#[test]
fn rounded_style_typing_shows_a_separator_under_the_title() {
    let state = FindFileState { phase: FindFilePhase::Typing, query: String::new(), cursor: 0, selection_anchor: None, content_query: String::new(), content_cursor: 0, content_selection_anchor: None, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, marked: Default::default(), pending: None, search_duration: None, results_capped: false, export_message: None };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let title_line_index = text.lines().position(|line| line.contains("Find file")).expect("title should render");
    let line_below = text.lines().nth(title_line_index + 1).unwrap();
    assert!(line_below.contains('─'), "a separator should sit right below the title: {line_below:?}");
}

/// Regression coverage for a real report: the `Typing` phase's
/// popup used a fixed height (5) sized for `Classic`'s tighter
/// chrome (just a 2-row border) -- under `Rounded`, whose border +
/// padding + title row eat 7 rows on their own, that left zero room
/// for the label/query/hint content, and the popup rendered
/// entirely blank. `draw_typing` now grows the popup by
/// `popup::chrome_extra_rows(style)` to keep the same 3 content rows
/// visible under either style.
#[test]
fn rounded_style_typing_phase_shows_the_label_and_hint_not_just_an_empty_box() {
    let state = FindFileState { phase: FindFilePhase::Typing, query: "abc".to_string(), cursor: 3, selection_anchor: None, content_query: String::new(), content_cursor: 0, content_selection_anchor: None, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, marked: Default::default(), pending: None, search_duration: None, results_capped: false, export_message: None };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("File name to find"), "label should be visible: {text}");
    assert!(text.contains("abc"), "typed query should be visible: {text}");
    assert!(text.contains("search"), "hint row should be visible: {text}");
}

/// Regression coverage for the Far-analogous "Text to find" field:
/// both labels and both typed values should be visible while typing,
/// and the cursor should follow whichever field is currently active.
#[test]
fn typing_phase_shows_both_fields_and_the_cursor_follows_the_active_one() {
    let mut state = FindFileState { phase: FindFilePhase::Typing, query: "read".to_string(), cursor: 4, selection_anchor: None, content_query: "todo".to_string(), content_cursor: 4, content_selection_anchor: None, active_field: FindFileField::Content, name_history_index: None, content_history_index: None, results: vec![], selected: 0, marked: Default::default(), pending: None, search_duration: None, results_capped: false, export_message: None };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    let mut cursor = None;
    terminal.draw(|frame| {
        cursor = draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("File name to find"), "name label should be visible: {text}");
    assert!(text.contains("Text to find"), "content label should be visible: {text}");
    assert!(text.contains("read"), "typed name query should be visible: {text}");
    assert!(text.contains("todo"), "typed content query should be visible: {text}");
    assert!(text.contains("switch field"), "hint row should mention Tab: {text}");

    let content_label_y = text.lines().position(|line| line.contains("Text to find")).unwrap() as u16;
    let cursor = cursor.expect("Typing phase should place a cursor");
    assert_eq!(cursor.y, content_label_y + 1, "cursor should sit on the content field's own value row, not the name field's");

    state.active_field = FindFileField::Name;
    let mut cursor_on_name = None;
    terminal.draw(|frame| {
        cursor_on_name = draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let name_label_y = {
        let buffer = terminal.backend().buffer();
        let text: String = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        text.lines().position(|line| line.contains("File name to find")).unwrap() as u16
    };
    assert_eq!(cursor_on_name.unwrap().y, name_label_y + 1, "cursor should follow active_field back to the name row");
}

/// Regression coverage for a real report: the popup used a fixed
/// absolute width regardless of the real terminal size, reading as
/// too narrow on a wide window. `draw_frame` is given
/// `popup::percent_width(area, 80)` now -- confirms the rendered
/// popup's own border actually sits at roughly 80% of two very
/// different terminal widths, not the same fixed column count
/// either time.
#[test]
fn typing_popup_width_scales_with_the_terminal_not_a_fixed_column_count() {
    let state = FindFileState { phase: FindFilePhase::Typing, query: String::new(), cursor: 0, selection_anchor: None, content_query: String::new(), content_cursor: 0, content_selection_anchor: None, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, marked: Default::default(), pending: None, search_duration: None, results_capped: false, export_message: None };
    let theme = Theme::dark();

    // The popup's top border row is the one row that contains both
    // rounded corner glyphs -- the gap between them is the popup's
    // own real rendered width.
    let popup_width = |terminal_width: u16| -> u16 {
        let backend = TestBackend::new(terminal_width, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| { draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded); }).unwrap();
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            let left = (0..buffer.area.width).find(|&x| buffer[(x, y)].symbol() == "╭");
            let right = (0..buffer.area.width).rev().find(|&x| buffer[(x, y)].symbol() == "╮");
            if let (Some(left), Some(right)) = (left, right) {
                return right - left + 1;
            }
        }
        panic!("no top border row found");
    };

    let narrow = popup_width(80);
    let wide = popup_width(160);
    assert!(wide > narrow + 40, "a much wider terminal should give a visibly wider popup, not the same fixed width (narrow={narrow}, wide={wide})");
}

/// Regression coverage for a real report: neither field's own text
/// selection was ever rendered at all -- only plain text. A selected
/// run should carry `popup::selected_text_style`'s own background,
/// the rest of the field's text shouldn't.
#[test]
fn a_selection_in_the_name_field_is_rendered_with_the_selection_background() {
    let state = FindFileState {
        phase: FindFilePhase::Typing,
        query: "abcdef".to_string(),
        cursor: 4,
        selection_anchor: Some(1),
        content_query: String::new(),
        content_cursor: 0,
        content_selection_anchor: None,
        active_field: FindFileField::Name,
        name_history_index: None,
        content_history_index: None,
        results: vec![],
        selected: 0,
        marked: Default::default(),
        pending: None,
        search_duration: None,
        results_capped: false,
        export_message: None,
    };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| { draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded); }).unwrap();
    let buffer = terminal.backend().buffer();

    let row_text = |y: u16| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>();
    let value_row = (0..buffer.area.height).find(|&y| row_text(y).contains("abcdef")).expect("query value row should render");
    let row_cells: Vec<String> = (0..buffer.area.width).map(|x| buffer[(x, value_row)].symbol().to_string()).collect();
    let start_x = row_cells.windows(6).position(|w| w.join("") == "abcdef").unwrap();

    let selection_style = popup::selected_text_style(&theme);
    assert_eq!(buffer[((start_x + 1) as u16, value_row)].bg, selection_style.bg.unwrap(), "\"b\" (inside the selection, [1,4)) should carry the selection background");
    assert_ne!(buffer[(start_x as u16, value_row)].bg, selection_style.bg.unwrap(), "\"a\" (before the selection) should not");
    assert_ne!(buffer[((start_x + 4) as u16, value_row)].bg, selection_style.bg.unwrap(), "\"e\" (after the selection) should not");
}

/// A non-empty content query should show up in the results title
/// too, alongside the name query -- both are part of what the
/// search actually ran with.
#[test]
fn results_title_includes_the_content_query_when_set() {
    let mut state = state_with(1, 0);
    state.content_query = "needle".to_string();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("needle"), "the content query should show up in the title: {text}");
}

/// Regression coverage for the real request: while a search is
/// still running (`FindFilePhase::Searching`), the popup should
/// show live progress and an `Esc`-to-cancel hint -- Far Manager's
/// own dialog shows both instead of blocking with nothing to look
/// at. Uses a real background search (`explorer::spawn_search`,
/// re-exported test-only) against a scratch directory with enough
/// files that it's very unlikely to have already finished by the
/// time this reads the popup's own text.
#[test]
fn searching_phase_shows_live_progress_and_a_cancel_hint() {
    let dir = crate::test_support::unique_scratch_dir("find-file-ui-searching");
    for i in 0..500 {
        std::fs::write(dir.join(format!("file_{i}.txt")), b"hi").unwrap();
    }
    let pending = crate::explorer::spawn_search(dir, "file".to_string(), String::new());
    let state = FindFileState { phase: FindFilePhase::Searching, query: "file".to_string(), cursor: 4, selection_anchor: None, content_query: String::new(), content_cursor: 0, content_selection_anchor: None, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, marked: Default::default(), pending: Some(pending), search_duration: None, results_capped: false, export_message: None };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Searching"), "should show a searching status: {text}");
    assert!(text.contains("visited"), "should show live progress: {text}");
    assert!(text.contains("cancel"), "should hint that Esc cancels: {text}");
}

/// `state.pending` should always be `Some` while `Searching`, but
/// `draw_searching` shouldn't panic even if that invariant were
/// ever somehow broken -- degrades to a plain status line instead.
#[test]
fn searching_phase_does_not_panic_with_no_pending_search() {
    let state = FindFileState { phase: FindFilePhase::Searching, query: String::new(), cursor: 0, selection_anchor: None, content_query: String::new(), content_cursor: 0, content_selection_anchor: None, active_field: FindFileField::Name, name_history_index: None, content_history_index: None, results: vec![], selected: 0, marked: Default::default(), pending: None, search_duration: None, results_capped: false, export_message: None };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Rounded);
    }).unwrap();
}

/// Regression coverage for the real request: the results popup
/// should show how long the search actually took, alongside the
/// result count -- Far Manager's own dialog shows both once a
/// search finishes.
#[test]
fn results_title_shows_the_result_count_and_search_duration() {
    let mut state = state_with(3, 0);
    state.search_duration = Some(std::time::Duration::from_millis(42));
    let text = rendered(&state);
    assert!(text.contains("3 results"), "should show the result count: {text}");
    assert!(text.contains("42ms"), "should show the elapsed time: {text}");
}

/// Regression coverage for the real report: a search that hit
/// exactly `find_file_max_results` used to render as a plain,
/// precise-looking count with no way to tell it wasn't the real
/// total -- `results_capped` should turn that into a "+" instead.
#[test]
fn results_title_shows_a_plus_when_results_are_capped() {
    let mut state = state_with(200, 0);
    state.results_capped = true;
    let text = rendered(&state);
    assert!(text.contains("200+ results"), "a capped result count should show a +: {text}");
}

#[test]
fn results_title_shows_singular_result_and_seconds_over_a_full_second() {
    let mut state = state_with(1, 0);
    state.search_duration = Some(std::time::Duration::from_millis(1500));
    let text = rendered(&state);
    assert!(text.contains("1 result "), "should use the singular form for exactly one result: {text}");
    assert!(text.contains("1.50s"), "should switch to seconds past the one-second mark: {text}");
}

/// Both `PopupStyle`s should render the same query/results content
/// -- only the chrome (border/padding/title placement) differs.
#[test]
fn classic_style_still_shows_the_query_and_results() {
    let state = state_with(3, 0);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal.draw(|frame| {
        draw_find_file(frame, frame.area(), &state, &theme, PopupStyle::Classic);
    }).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("file_0000"));
    assert!(text.contains("Find file"));
}
