use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::Index2;

use super::*;
use crate::test_support::{ctrl_key, key, shift_key, unique_scratch_dir};
use crate::theming::Theme;

/// Writes `contents` to a scratch file and opens it, so tests can
/// exercise `Editor` without a fixture directory. Returns the path
/// too, so tests can read back what `save` wrote.
fn open_test_editor(contents: &str) -> (Editor, PathBuf) {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, contents).expect("write test fixture file");
    let editor = Editor::open(path.clone(), None, EditorKeymapMode::Standard).expect("open test fixture file");
    (editor, path)
}


/// Same as `open_test_editor`, but under a `.rs` name -- for tests that
/// need real syntax highlighting to actually be active (`file.txt`
/// above resolves to Plain Text, which colors nothing).
fn open_test_rust_editor(contents: &str) -> Editor {
    let path = unique_scratch_dir("editor").join("file.rs");
    fs::write(&path, contents).expect("write test fixture file");
    Editor::open(path, None, EditorKeymapMode::Standard).expect("open test fixture file")
}


mod dirty_and_save;
mod highlighting;
mod search_scroll;
mod keymap_modes;
mod line_selection;
mod rendering;
mod selection;
mod undo_and_paste;
