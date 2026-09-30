use super::*;
use crate::test_support::test_app;

#[test]
fn opens_the_editor_and_a_linked_preview_with_the_editor_active() {
    let dir = unique_scratch_dir("markdown-preview-open");
    fs::write(dir.join("readme.md"), "# hi\n").unwrap();
    let mut app = test_app(dir.clone());
    app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "readme.md").unwrap();

    open_edit_preview(&mut app);

    assert!(matches!(app.mode, Mode::Editing(_)));
    assert!(app.markdown_edit_preview.is_some(), "should link a live preview alongside the editor");
    assert_eq!(app.active, 0, "the left panel (the editor) should start active, ready to type into");
}

#[test]
fn is_a_noop_on_a_non_markdown_file() {
    let dir = unique_scratch_dir("markdown-preview-open");
    fs::write(dir.join("notes.txt"), "hi").unwrap();
    let mut app = test_app(dir);
    app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "notes.txt").unwrap();

    open_edit_preview(&mut app);

    assert!(matches!(app.mode, Mode::Browsing));
    assert!(app.markdown_edit_preview.is_none());
}
