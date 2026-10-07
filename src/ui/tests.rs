use ratatui::{backend::TestBackend, layout::Rect, style::Color, widgets::List, Terminal};

use super::*;
use crate::test_support::buffer_text;
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

/// While the editor is open, `draw` reports the panels' real layout, not a
/// `(1, 1)` placeholder -- it's applied every frame, so the placeholder
/// crammed the panels into one column. History:
/// docs/history/event-loop.md.
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

    let text = buffer_text(terminal.backend().buffer());
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
