use ratatui::{backend::TestBackend, layout::Rect, style::Color, widgets::List, Terminal};

use super::*;
use crate::explorer::Entry;
use crate::theming::Theme;

/// Reads back the visible text of row `y`, column start inclusive,
/// by concatenating each cell's symbol — the same shape a real
/// terminal would show, so an `x` found in this string lines up
/// with an actual on-screen column.
fn rendered_row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn render_function_keys(width: u16, alt: bool) -> String {
    let backend = TestBackend::new(width, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal
        .draw(|frame| draw_function_keys(frame, Rect::new(0, 0, width, 1), &theme, alt))
        .unwrap();
    rendered_row(&terminal, 0)
}

/// The actual reported bug: the row used to be one flowing `Line`
/// with each label's own natural width, so switching `Alt` (e.g.
/// `"Folder"` <-> `"Find"`, six characters vs. four) visibly
/// reflowed every key from F7 onward. Column layout fixes this —
/// this test renders both label sets into a real buffer and checks
/// that every later key's own column (found by its own `"F<n> "`
/// prefix) starts at the exact same `x` regardless of which set is
/// showing, not just that the layout math looks right on paper.
#[test]
fn switching_alt_labels_does_not_shift_later_columns() {
    let default_row = render_function_keys(120, false);
    let alt_row = render_function_keys(120, true);

    for key in ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"] {
        let default_pos = default_row.find(key).unwrap_or_else(|| panic!("{key} missing from default row: {default_row:?}"));
        let alt_pos = alt_row.find(key).unwrap_or_else(|| panic!("{key} missing from alt row: {alt_row:?}"));
        assert_eq!(default_pos, alt_pos, "{key} shifted position when switching labels (default row: {default_row:?}, alt row: {alt_row:?})");
    }
}

/// The row should use the full width available to it (10 equal
/// columns spanning `area`), not just as much as the longest
/// label set happens to need — `function_key_columns`'s last
/// column should reach the right edge.
#[test]
fn columns_span_the_full_available_width() {
    let columns = function_key_columns(Rect::new(0, 0, 100, 1));
    let last = columns.last().unwrap();
    assert_eq!(last.x + last.width, 100, "last column should reach the right edge: {columns:?}");
}

/// Regression test for a real reported bug: `"Bookmarks"` (9
/// characters) visibly ran into the next column with no gap, since
/// equal-width columns reserve no padding between them. Caps every
/// label (both label sets) at Far Manager's own 6-character
/// convention (`"UserMn"`, `"MkFold"`, `"ConfMn"`, ...) so this
/// can't silently regress if a label is ever lengthened.
#[test]
fn labels_stay_within_the_six_character_budget() {
    for &(key, label) in DEFAULT_LABELS.iter().chain(ALT_LABELS.iter()) {
        assert!(label.len() <= 6, "{key}'s label {label:?} is {} characters, over the 6-character budget", label.len());
    }
}

/// The actual reported bug: closing the built-in editor showed a
/// single entry crammed into one narrow column for one whole extra
/// frame. Root cause was `draw`'s own early-return branches (the
/// editor, Compare, ...) reporting a hardcoded `(1, 1)` layout instead
/// of each panel's own already-known `(columns, visible_rows)` --
/// `main.rs::run`'s loop applies whatever this function returns
/// straight onto both panels via `Panel::set_columns`/`set_visible_rows`
/// on *every* frame, including every frame the editor stays open, so a
/// hardcoded placeholder was clobbering the real values down to a
/// forced single column/row the whole time, not just leaving them
/// stale for one frame. This drives `draw` itself while in
/// `Mode::Editing`, with the panels pre-seeded to real, multi-column
/// values, and checks the returned layout still reports those same
/// values back -- not `(1, 1)`.
#[test]
fn editing_mode_reports_each_panels_own_unchanged_layout_not_a_placeholder() {
    use crate::app::Mode;
    use crate::editor::{Editor, EditorKeymapMode};
    use crate::test_support::{test_app, unique_scratch_dir};

    let dir = unique_scratch_dir("ui-editing-layout");
    let file = dir.join("shell.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();

    let mut app = test_app(dir);
    app.panels[0].set_columns(3);
    app.panels[0].set_visible_rows(7);
    app.panels[1].set_columns(2);
    app.panels[1].set_visible_rows(5);
    app.mode = Mode::Editing(Editor::open(file, None, EditorKeymapMode::Standard).expect("open test fixture file"));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut layout = [(0usize, 0usize); 2];
    terminal
        .draw(|frame| {
            layout = draw(frame, &mut app).0;
        })
        .unwrap();

    assert_eq!(layout, [(3, 7), (2, 5)], "editing shouldn't report a placeholder layout that would clobber the panels' real columns/rows");
}

#[test]
fn draw_info_popup_shows_the_message_and_the_dismiss_hint() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let theme = Theme::dark();
    terminal
        .draw(|frame| draw_info_popup(frame, frame.area(), "FarMenu.ini backed up as FarMenu.ini.bak", &theme, crate::theming::PopupStyle::Rounded))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("FarMenu.ini.bak"));
    assert!(text.contains("any key"));
}

fn test_entry(name: &str) -> Entry {
    Entry { name: name.to_string(), is_dir: false, size: 0, modified: None }
}

/// A theme that doesn't set `selection_text` (every built-in scheme
/// today, and any Windows Terminal JSON downloaded as-is) must keep
/// the selected row's own file-type color, not have it silently
/// replaced -- the deliberate "file-type color survives being
/// selected" convention (`.claude/rules/litastum-theming.md`).
#[test]
fn selected_row_keeps_its_own_color_when_theme_has_no_selection_text_override() {
    let theme = Theme::dark();
    assert_eq!(theme.selection_text, None);
    let item = build_list_item(&test_entry("notes.txt"), true, false, true, &theme);

    let backend = TestBackend::new(20, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| frame.render_widget(List::new([item]), frame.area())).unwrap();

    let fg = terminal.backend().buffer()[(0, 0)].fg;
    assert_eq!(fg, theme.text, "an ordinary file's own color (theme.text) should be unchanged");
}

/// The actual point of the whole `selection_text` feature: a scheme
/// that *does* set it (requested directly, to keep text readable over
/// a deliberately bright selection background) overrides the selected
/// row's text color.
#[test]
fn selected_row_uses_the_theme_override_color_when_set() {
    let mut theme = Theme::dark();
    theme.selection_text = Some(Color::Rgb(0, 0, 0));
    let item = build_list_item(&test_entry("notes.txt"), true, false, true, &theme);

    let backend = TestBackend::new(20, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| frame.render_widget(List::new([item]), frame.area())).unwrap();

    let fg = terminal.backend().buffer()[(0, 0)].fg;
    assert_eq!(fg, Color::Rgb(0, 0, 0), "the selected row should use the theme's override color");
}
