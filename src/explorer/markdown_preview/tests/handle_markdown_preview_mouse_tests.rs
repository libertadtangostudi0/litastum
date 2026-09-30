use super::*;
use crate::editor::{Editor, EditorKeymapMode};
use crate::test_support::test_app;

/// A left click with no `Ctrl` (or `Ctrl`+click landing on a line
/// with no link) must never call `system_open::open` -- these tests
/// would spawn a *real* OS process (a real browser) if that guard
/// were ever removed, so they deliberately stick to cases with no
/// link to actually open.
fn app_in_preview() -> App {
    let dir = unique_scratch_dir("markdown-preview-mouse");
    let path = dir.join("readme.md");
    fs::write(&path, "no link here\n\n[a link](https://example.com)\n").unwrap();
    let mut app = test_app(dir);
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(0, 0, 80, 24);
    let editor = Editor::open(path, None, EditorKeymapMode::Standard).unwrap();
    app.markdown_edit_preview = Some(state);
    app.mode = Mode::Editing(editor);
    app
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent { kind, column, row, modifiers }
}

#[test]
fn plain_click_without_ctrl_does_not_scroll_or_panic() {
    let mut app = app_in_preview();

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::NONE));

    assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 0);
}

#[test]
fn ctrl_click_on_a_line_with_no_link_does_nothing_observable() {
    let mut app = app_in_preview();

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

    assert!(matches!(app.mode, Mode::Editing(_)), "should not have crashed or changed mode");
}

/// Regression coverage for the real bug: an in-document anchor link
/// must not be handed to `system_open::open` at all (which would
/// spawn a real process here if this guard broke) --
/// `resolve_link_target` already covers the resolution logic in
/// isolation, this confirms the mouse handler actually calls it
/// before ever reaching `system_open::open`.
#[test]
fn ctrl_click_on_an_anchor_link_does_not_open_anything() {
    let dir = unique_scratch_dir("markdown-preview-mouse");
    let path = dir.join("readme.md");
    fs::write(&path, "[Jump](#section)\n").unwrap();
    let mut app = test_app(dir);
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(0, 0, 80, 24);
    state.set_visible_row_links(vec![vec![(0, "Jump".chars().count() as u16, "#section".to_string())]]);
    let editor = Editor::open(path, None, EditorKeymapMode::Standard).unwrap();
    app.markdown_edit_preview = Some(state);
    app.mode = Mode::Editing(editor);

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

    assert!(matches!(app.mode, Mode::Editing(_)), "should not have crashed or changed mode");
}

/// A click on a link that can't be opened still leaves a message, named by
/// its label -- silence looked like the click never registered.
/// History: docs/history/markdown-preview.md.
#[test]
fn ctrl_click_on_an_anchor_link_sets_an_explanatory_message() {
    let dir = unique_scratch_dir("markdown-preview-mouse");
    let path = dir.join("readme.md");
    fs::write(&path, "[Jump](#section)\n").unwrap();
    let mut app = test_app(dir);
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(0, 0, 80, 24);
    state.set_visible_row_links(vec![vec![(0, "Jump".chars().count() as u16, "#section".to_string())]]);
    let editor = Editor::open(path, None, EditorKeymapMode::Standard).unwrap();
    app.markdown_edit_preview = Some(state);
    app.mode = Mode::Editing(editor);

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

    let message = app.markdown_edit_preview.as_ref().unwrap().link_message().expect("should have set a message explaining the click's outcome");
    assert!(message.contains("Jump"), "message should name the label of the link that was clicked: {message:?}");
}

#[test]
fn ctrl_click_on_a_line_with_no_link_sets_a_no_link_message() {
    let mut app = app_in_preview();

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

    assert!(app.markdown_edit_preview.as_ref().unwrap().link_message().is_some(), "should say something, not stay silent");
}

#[test]
fn scroll_down_advances_without_needing_ctrl() {
    let mut app = app_in_preview();

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown, 0, 0, KeyModifiers::NONE));

    assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 1);
}

#[test]
fn scroll_up_stops_at_zero() {
    let mut app = app_in_preview();

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp, 0, 0, KeyModifiers::NONE));

    assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 0);
}

#[test]
fn is_a_noop_outside_editing_mode() {
    let mut app = app_in_preview();
    app.mode = Mode::Browsing;

    handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown, 0, 0, KeyModifiers::NONE));

    assert!(matches!(app.mode, Mode::Browsing));
}
