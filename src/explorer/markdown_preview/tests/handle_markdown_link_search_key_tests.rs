use super::*;
use crate::editor::{Editor, EditorKeymapMode};
use crate::test_support::{key, test_app};

/// The link search open over an editor+preview session.
fn app_in_search() -> App {
    let dir = unique_scratch_dir("markdown-link-search-keys");
    let path = dir.join("readme.md");
    fs::write(&path, "[Anthropic](https://anthropic.com)\n\n[Contributing](CONTRIBUTING.md)\n").unwrap();
    let mut app = test_app(dir);
    let preview = MarkdownPreviewState::open(&path).unwrap();
    let links = preview.links();
    let editor = Editor::open(path, None, EditorKeymapMode::Standard).unwrap();
    app.markdown_edit_preview = Some(preview);
    app.mode = Mode::Editing(editor);
    app.overlay = Some(Overlay::MarkdownLinkSearch(MarkdownLinkSearchState::new(links)));
    app
}

#[test]
fn typing_filters_the_list() {
    let mut app = app_in_search();

    handle_markdown_link_search_key(&mut app, key(KeyCode::Char('a')));

    let Some(Overlay::MarkdownLinkSearch(search)) = &app.overlay else { panic!("expected Overlay::MarkdownLinkSearch") };
    assert_eq!(search.query(), "a");
}

#[test]
fn esc_cancels_back_to_editing_with_the_preview_still_linked() {
    let mut app = app_in_search();

    handle_markdown_link_search_key(&mut app, key(KeyCode::Esc));

    assert!(matches!(app.mode, Mode::Editing(_)));
    assert!(app.markdown_edit_preview.is_some());
}

/// The actual end-to-end point of the whole feature: `Enter`
/// resolves and reacts to the *selected* result, then returns to
/// editing with a message recording what happened on the linked
/// preview -- naming the link's *label* ("Contributing"), not its
/// raw URL, per `MarkdownPreviewState::link_message`'s own field
/// doc comment. Deliberately selects the *relative, missing-file*
/// link (`Down` once), not the real absolute URL at index 0 -- a
/// test that actually opened a real URL would spawn a real OS
/// process (a real browser) every time this suite runs.
#[test]
fn enter_opens_the_selected_link_and_returns_to_editing() {
    let mut app = app_in_search();
    handle_markdown_link_search_key(&mut app, key(KeyCode::Down)); // select "Contributing" (CONTRIBUTING.md, doesn't exist here)

    handle_markdown_link_search_key(&mut app, key(KeyCode::Enter));

    assert!(matches!(app.mode, Mode::Editing(_)));
    let message = app.markdown_edit_preview.as_ref().unwrap().link_message().expect("should have recorded what Enter did");
    assert!(message.contains("Contributing"), "message should name the label of the link that was selected: {message:?}");
}

#[test]
fn enter_with_no_matches_just_returns_to_editing() {
    let mut app = app_in_search();
    for c in "nonexistent".chars() {
        handle_markdown_link_search_key(&mut app, key(KeyCode::Char(c)));
    }

    handle_markdown_link_search_key(&mut app, key(KeyCode::Enter));

    assert!(matches!(app.mode, Mode::Editing(_)));
    assert_eq!(app.markdown_edit_preview.as_ref().unwrap().link_message(), None, "nothing was selected, so nothing should be reported either");
}

#[test]
fn is_a_noop_outside_link_search_mode() {
    let mut app = app_in_search();
    app.mode = Mode::Browsing;

    handle_markdown_link_search_key(&mut app, key(KeyCode::Char('a')));

    assert!(matches!(app.mode, Mode::Browsing));
}
