use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::text_field::TextField;

/// A pane's path title turned into a text field (a click on it, or
/// `Ctrl+L`), as in Araxis: `Enter` loads the typed path into that pane,
/// `Esc` puts the title back.
pub struct PathEdit {
    pub field: TextField,
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
        let mut field = TextField::with_text(path.to_string_lossy().into_owned());
        field.move_to_end();
        Self { field }
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
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

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
}
