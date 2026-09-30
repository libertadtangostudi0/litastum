use super::*;
use crate::editor::{Editor, EditorKeymapMode};
use crate::test_support::{key, test_app};

/// Builds a combined editor+preview session (`App::markdown_edit_preview`
/// linked to `Mode::Editing`), focused on the preview half
/// (`app.active = 1`) -- the state `event_loop::keys::handle_key_event`
/// routes to `handle_markdown_edit_preview_key` in the first place.
fn app_in_preview(content: &str) -> App {
    let dir = unique_scratch_dir("markdown-preview-keys");
    let path = dir.join("readme.md");
    fs::write(&path, content).unwrap();
    let mut app = test_app(dir);
    let preview = MarkdownPreviewState::open(&path).unwrap();
    let editor = Editor::open(path, None, EditorKeymapMode::Standard).unwrap();
    app.markdown_edit_preview = Some(preview);
    app.mode = Mode::Editing(editor);
    app.active = 1;
    app
}

#[test]
fn down_scrolls_the_preview_forward() {
    let mut app = app_in_preview("line one\n\nline two\n\nline three\n");

    handle_markdown_edit_preview_key(&mut app, key(KeyCode::Down)).unwrap();

    assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 1);
}

/// `Esc` closes the *whole* editor+preview session (`editor::close_editor_or_confirm`),
/// not just the preview half -- an unmodified, freshly opened editor
/// has no unsaved changes, so this returns straight to `Mode::Browsing`
/// with the linked preview cleared.
#[test]
fn esc_closes_the_whole_session() {
    let mut app = app_in_preview("hello\n");

    handle_markdown_edit_preview_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
    assert!(app.markdown_edit_preview.is_none(), "closing should drop the linked preview too");
}

#[test]
fn f3_again_also_closes_the_whole_session() {
    let mut app = app_in_preview("hello\n");

    handle_markdown_edit_preview_key(&mut app, key(KeyCode::F(3))).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}

#[test]
fn is_a_noop_without_a_linked_preview() {
    let dir = unique_scratch_dir("markdown-preview-keys");
    let path = dir.join("readme.md");
    fs::write(&path, "hello\n").unwrap();
    let mut app = test_app(dir);
    app.mode = Mode::Editing(Editor::open(path, None, EditorKeymapMode::Standard).unwrap());

    handle_markdown_edit_preview_key(&mut app, key(KeyCode::Down)).unwrap();

    assert!(app.markdown_edit_preview.is_none());
}

/// The actual point of the whole keyboard-search feature: `l` opens
/// it over the editor, which stays open underneath.
#[test]
fn l_opens_the_link_search_when_the_document_has_links() {
    let mut app = app_in_preview("[Anthropic](https://anthropic.com)\n");

    handle_markdown_edit_preview_key(&mut app, key(KeyCode::Char('l'))).unwrap();

    assert!(matches!(app.overlay, Some(Overlay::MarkdownLinkSearch(..))));
    assert!(matches!(app.mode, Mode::Editing(_)));
}

#[test]
fn l_is_a_noop_when_the_document_has_no_links() {
    let mut app = app_in_preview("no links here\n");

    handle_markdown_edit_preview_key(&mut app, key(KeyCode::Char('l'))).unwrap();

    assert!(app.overlay.is_none(), "should stay put, nothing to search");
}
