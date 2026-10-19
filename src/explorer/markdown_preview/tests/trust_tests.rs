use super::*;
use super::trust::{request_open_link, test_store};
use crate::editor::{Editor, EditorKeymapMode};
use crate::test_support::{key, test_app};

/// The link's text says one place, it goes to another: the question must
/// show where it goes.
fn app_with_link() -> App {
    let dir = unique_scratch_dir("markdown-preview-trust");
    let path = dir.join("readme.md");
    fs::write(&path, "[Docs](https://example.com/docs)\n").unwrap();
    let mut app = test_app(dir);
    app.markdown_edit_preview = Some(MarkdownPreviewState::open(&path).unwrap());
    app.mode = Mode::Editing(Editor::open(path, None, EditorKeymapMode::Standard).unwrap());
    app
}

const URL: &str = "https://example.com/docs";

fn asks(app: &App) -> bool {
    matches!(&app.overlay, Some(Overlay::ConfirmOpenLink(pending)) if pending.destination() == URL && pending.label() == "Docs")
}

#[test]
fn an_untrusted_link_asks_and_esc_opens_nothing() {
    let mut app = app_with_link();

    request_open_link(&mut app, URL);
    assert!(asks(&app));
    assert!(test_store::launched().is_empty(), "nothing opened before the answer");

    handle_confirm_open_link_key(&mut app, key(KeyCode::Esc)).unwrap();
    assert!(app.overlay.is_none());
    assert!(test_store::launched().is_empty());
}

#[test]
fn y_opens_it_once_and_the_next_time_asks_again() {
    let mut app = app_with_link();

    request_open_link(&mut app, URL);
    handle_confirm_open_link_key(&mut app, key(KeyCode::Char('y'))).unwrap();
    assert!(app.overlay.is_none());
    assert_eq!(test_store::launched(), vec![URL]);

    request_open_link(&mut app, URL);
    assert!(asks(&app), "not trusted by opening it once");
}

#[test]
fn t_trusts_it_and_then_it_opens_without_asking() {
    let mut app = app_with_link();

    request_open_link(&mut app, URL);
    handle_confirm_open_link_key(&mut app, key(KeyCode::Char('t'))).unwrap();
    assert_eq!(test_store::launched(), vec![URL]);

    request_open_link(&mut app, URL);
    assert!(app.overlay.is_none(), "trusted: no question");
    assert_eq!(test_store::launched(), vec![URL, URL]);
}

#[test]
fn another_key_leaves_the_question_up() {
    let mut app = app_with_link();

    request_open_link(&mut app, URL);
    handle_confirm_open_link_key(&mut app, key(KeyCode::Enter)).unwrap();

    assert!(asks(&app));
    assert!(test_store::launched().is_empty());
}
