use std::fs;
use std::path::PathBuf;

use super::*;
use crate::app::Overlay;
use crate::editor::{Editor, EditorKeymapMode};
use crate::test_support::{ctrl_key, key, shift_key, test_app, unique_scratch_dir};

/// A real `App` (no terminal needed) in `Mode::Editing`, with the
/// panel rooted in the same scratch directory as the opened file so
/// `close_editor_or_confirm`'s `app.active_panel().reload()` has
/// somewhere real to reload.
fn open_editor_app(contents: &str) -> (App, PathBuf) {
    let dir = unique_scratch_dir("editor-keymap");
    let file_path = dir.join("file.txt");
    fs::write(&file_path, contents).expect("write test fixture file");

    let editor = Editor::open(file_path.clone(), None, EditorKeymapMode::Standard).expect("open test fixture file");
    let mut app = test_app(dir);
    app.mode = Mode::Editing(editor);
    (app, file_path)
}

mod resolve_editor_key_tests;
mod handle_editor_key_tests;
mod handle_search_key_tests;
mod handle_confirm_discard_key_tests;
