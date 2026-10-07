//! An editor's or a panel's path title turned into a text field, as in
//! Araxis -- F4, Compare, the conflict resolver and the file panels share
//! it. The drawing is `ui::path_edit`.

use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::editor::Editor;
use crate::text_field::TextField;

/// The field over a path title: `Enter` acts on the typed path
/// (`PathPurpose`), `Esc` puts the title back.
pub struct PathEdit {
    pub field: TextField,
    pub purpose: PathPurpose,
    /// A save-as target that already exists and was reported once: the
    /// next `Enter` on the same path overwrites it.
    overwrite_confirmed: Option<PathBuf>,
}

/// What `Enter` does with the typed path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPurpose {
    /// `Ctrl+L` or a click on the title: load it into the pane.
    Open,
    /// `Shift+F2`: save the pane's text there, as in Far.
    SaveAs,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PathEditKey {
    Editing,
    Cancel,
    Submit(PathBuf),
}

impl PathEdit {
    /// Starts with the pane's path and the caret at its end, by the file
    /// name.
    pub fn new(path: &Path) -> Self {
        Self::with_purpose(path, PathPurpose::Open)
    }

    pub fn save_as(path: &Path) -> Self {
        Self::with_purpose(path, PathPurpose::SaveAs)
    }

    fn with_purpose(path: &Path, purpose: PathPurpose) -> Self {
        let mut field = TextField::with_text(path.to_string_lossy().into_owned());
        field.move_to_end();
        Self { field, purpose, overwrite_confirmed: None }
    }

    pub fn key(&mut self, key: KeyEvent) -> PathEditKey {
        match key.code {
            KeyCode::Esc => PathEditKey::Cancel,
            KeyCode::Enter => PathEditKey::Submit(PathBuf::from(self.field.text().trim().trim_matches('"'))),
            _ => {
                self.field.apply_key(key);
                PathEditKey::Editing
            }
        }
    }

    /// Saves `editor` as `typed` (relative to the editor's own directory),
    /// which becomes its file. Another file already there is overwritten
    /// only on a second `Enter` with the same path; the first one fails
    /// with `AlreadyExists`, saying so. Errors read as a whole notice.
    pub fn save_editor_as(&mut self, editor: &mut Editor, typed: PathBuf) -> io::Result<()> {
        let target = match editor.path().parent() {
            Some(dir) if typed.is_relative() => dir.join(&typed),
            _ => typed,
        };
        let replaces_another = target.exists() && target != editor.path();
        if replaces_another && self.overwrite_confirmed.as_ref() != Some(&target) {
            self.overwrite_confirmed = Some(target.clone());
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists -- Enter again to overwrite it", target.display())));
        }
        editor.save_as(target).map_err(|err| io::Error::new(err.kind(), format!("Save failed: {err}")))
    }
}


/// Puts "Can't open" in front of a failed load, so callers can show any
/// path field failure as it is.
pub(crate) fn open_failed(err: io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("Can't open: {err}"))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{key, unique_scratch_dir};

    #[test]
    fn starts_with_the_path_and_the_caret_at_its_end() {
        let edit = PathEdit::new(Path::new("src/a.rs"));
        assert_eq!(edit.field.text(), "src/a.rs");
        assert_eq!(edit.field.cursor(), 8);
    }

    #[test]
    fn enter_submits_the_typed_path_without_quotes_or_spaces() {
        let mut edit = PathEdit::new(Path::new("src/a.rs"));
        edit.field.set_text(" \"src/b.rs\" ");
        assert_eq!(edit.key(key(KeyCode::Enter)), PathEditKey::Submit(PathBuf::from("src/b.rs")));
    }

    #[test]
    fn typing_edits_and_esc_cancels() {
        let mut edit = PathEdit::new(Path::new("a"));
        assert_eq!(edit.key(key(KeyCode::Char('b'))), PathEditKey::Editing);
        assert_eq!(edit.field.text(), "ab");
        assert_eq!(edit.key(key(KeyCode::Esc)), PathEditKey::Cancel);
    }

    fn open_editor(dir: &Path, text: &str) -> Editor {
        let path = dir.join("a.txt");
        std::fs::write(&path, text).unwrap();
        Editor::open(path, None, EditorKeymapMode::Standard).unwrap()
    }

    #[test]
    fn save_as_writes_a_new_file_and_the_editor_moves_to_it() {
        let dir = unique_scratch_dir("save-as");
        let mut editor = open_editor(&dir, "one\n");
        editor.input(key(KeyCode::Char('!')));
        let mut edit = PathEdit::save_as(editor.path());

        edit.save_editor_as(&mut editor, PathBuf::from("b.txt")).unwrap();

        assert_eq!(editor.path(), dir.join("b.txt"), "relative to the editor's own directory");
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "!one\n");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n", "the original stays");
        assert!(!editor.is_dirty());
    }

    #[test]
    fn an_existing_file_is_overwritten_only_on_the_second_enter() {
        let dir = unique_scratch_dir("save-as");
        std::fs::write(dir.join("b.txt"), "keep\n").unwrap();
        let mut editor = open_editor(&dir, "one\n");
        let mut edit = PathEdit::save_as(editor.path());

        let err = edit.save_editor_as(&mut editor, PathBuf::from("b.txt")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "keep\n");

        edit.save_editor_as(&mut editor, PathBuf::from("b.txt")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "one\n");
    }

    #[test]
    fn save_as_onto_its_own_path_just_saves() {
        let dir = unique_scratch_dir("save-as");
        let mut editor = open_editor(&dir, "one\n");
        editor.input(key(KeyCode::Char('!')));
        let mut edit = PathEdit::save_as(editor.path());

        edit.save_editor_as(&mut editor, dir.join("a.txt")).unwrap();

        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "!one\n");
    }

    #[test]
    fn a_failed_save_as_keeps_the_old_path() {
        let dir = unique_scratch_dir("save-as");
        let mut editor = open_editor(&dir, "one\n");
        let mut edit = PathEdit::save_as(editor.path());

        assert!(edit.save_editor_as(&mut editor, PathBuf::from("no/such/dir/b.txt")).is_err());

        assert_eq!(editor.path(), dir.join("a.txt"));
    }
}
