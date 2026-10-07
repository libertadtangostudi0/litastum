//! An editor's or a panel's path title turned into a text field, as in
//! Araxis -- F4, Compare, the conflict resolver and the file panels share
//! it. The drawing is `ui::path_edit`.

use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::editor::Editor;
use crate::text_field::TextField;

mod complete;

pub use complete::Completions;
pub(crate) use complete::matching_entries;

/// The field over a path title: `Enter` acts on the typed path
/// (`PathPurpose`), `Esc` puts the title back. `Tab` completes the path,
/// with a list under the field when several entries match.
pub struct PathEdit {
    pub field: TextField,
    pub purpose: PathPurpose,
    /// The open `Tab` list: `Up`/`Down` move, `Tab`/`Enter` accept, `Esc`
    /// closes just the list; typing filters it, and it stays open (saying
    /// "no matches") when nothing is left. `Enter` on an empty list acts
    /// as on the field.
    pub completions: Option<Completions>,
    /// What a relative path is completed against: the panel's directory,
    /// or an editor's file's directory.
    base: PathBuf,
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
        let base = if path.is_dir() { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
        Self { field, purpose, completions: None, base, overwrite_confirmed: None }
    }

    /// What a relative typed path is taken against.
    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn key(&mut self, key: KeyEvent) -> PathEditKey {
        if let Some(list) = self.completions.as_mut() {
            match key.code {
                KeyCode::Up => list.up(),
                KeyCode::Down => list.down(),
                // Nothing to pick: falls through to the field below.
                KeyCode::Enter if list.items.is_empty() => self.completions = None,
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(accepted) = list.accepted() {
                        self.field.set_text(accepted);
                    }
                    self.completions = None;
                    return PathEditKey::Editing;
                }
                KeyCode::Esc => {
                    self.completions = None;
                    return PathEditKey::Editing;
                }
                _ => {
                    self.field.apply_key(key);
                    self.completions = Some(Completions::list(self.field.text(), &self.base));
                    return PathEditKey::Editing;
                }
            }
            if self.completions.is_some() {
                return PathEditKey::Editing;
            }
        }
        match key.code {
            KeyCode::Esc => PathEditKey::Cancel,
            KeyCode::Enter => PathEditKey::Submit(PathBuf::from(self.field.text().trim().trim_matches('"'))),
            KeyCode::Tab => {
                match complete::tab(self.field.text(), &self.base) {
                    complete::Tab::Complete(text) => self.field.set_text(text),
                    complete::Tab::Choose(text, list) => {
                        self.field.set_text(text);
                        self.completions = Some(list);
                    }
                }
                PathEditKey::Editing
            }
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

    /// Requested: Tab in a path field completes it, with a list to pick
    /// from when several entries match.
    #[test]
    fn tab_opens_a_list_that_arrows_and_enter_pick_from() {
        let dir = unique_scratch_dir("path-edit-tab");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        let mut edit = PathEdit::new(&dir);
        edit.field.set_text(format!("{}{sep}s", dir.display()));

        assert_eq!(edit.key(key(KeyCode::Tab)), PathEditKey::Editing);
        assert_eq!(edit.completions.as_ref().map(|list| list.items.len()), Some(2));
        edit.key(key(KeyCode::Down));
        assert_eq!(edit.key(key(KeyCode::Enter)), PathEditKey::Editing, "Enter picks from the list, it doesn't submit");

        assert_eq!(edit.field.text(), format!("{}{sep}src{sep}", dir.display()));
        assert!(edit.completions.is_none());
    }

    #[test]
    fn typing_filters_the_list_and_esc_closes_only_the_list() {
        let dir = unique_scratch_dir("path-edit-tab");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        let mut edit = PathEdit::new(&dir);
        edit.field.set_text("s");

        edit.key(key(KeyCode::Tab));
        edit.key(key(KeyCode::Char('r')));
        assert_eq!(edit.completions.as_ref().map(|list| list.items.len()), Some(1), "only src is left");

        assert_eq!(edit.key(key(KeyCode::Esc)), PathEditKey::Editing);
        assert!(edit.completions.is_none());
        assert_eq!(edit.key(key(KeyCode::Esc)), PathEditKey::Cancel, "the next Esc closes the field");
    }

    /// Requested: the list must work with a partly typed name -- typing
    /// past every match and back again keeps it, and says when nothing
    /// matches rather than closing.
    #[test]
    fn the_list_follows_partial_typing_both_ways() {
        let dir = unique_scratch_dir("path-edit-tab");
        for name in ["packaging", "packages", "src"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let sep = std::path::MAIN_SEPARATOR;
        let mut edit = PathEdit::new(&dir);
        edit.field.set_text(format!("{}{sep}pa", dir.display()));
        let names = |edit: &PathEdit| edit.completions.as_ref().map(|list| list.items.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>());

        edit.key(key(KeyCode::Tab));
        assert_eq!(edit.field.text(), format!("{}{sep}packag", dir.display()), "extended to the shared start");
        assert_eq!(names(&edit), Some(vec!["packages".to_string(), "packaging".into()]));

        edit.key(key(KeyCode::Char('x')));
        assert_eq!(names(&edit), Some(vec![]), "no matches: the list stays, empty");
        edit.key(key(KeyCode::Backspace));
        edit.key(key(KeyCode::Char('i')));
        assert_eq!(names(&edit), Some(vec!["packaging".to_string()]));

        edit.key(key(KeyCode::Tab));
        assert_eq!(edit.field.text(), format!("{}{sep}packaging{sep}", dir.display()));
        assert!(edit.completions.is_none());
    }

    #[test]
    fn tab_with_nothing_to_complete_says_so_and_enter_then_submits() {
        let dir = unique_scratch_dir("path-edit-tab");
        let mut edit = PathEdit::new(&dir);
        edit.field.set_text("zzz");

        edit.key(key(KeyCode::Tab));
        assert!(edit.completions.as_ref().is_some_and(|list| list.items.is_empty()));
        assert_eq!(edit.field.text(), "zzz");

        assert_eq!(edit.key(key(KeyCode::Enter)), PathEditKey::Submit(PathBuf::from("zzz")), "an empty list doesn't swallow Enter");
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
