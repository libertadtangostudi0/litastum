use crate::editor::Editor;

use super::diff::{compute, DiffLines};

/// Two editors' line diff, kept until either text changes
/// (`Editor::revision`). Compare and the conflict resolver used to build
/// both texts and diff them on every frame -- ~1.3 s a frame for two
/// 20k-line files in a debug build -- so moving the caret crawled, worse
/// the more lines a small font put on screen.
#[derive(Default)]
pub struct DiffCache {
    revisions: Option<(u64, u64)>,
    diff: Option<(DiffLines, DiffLines)>,
}

impl DiffCache {
    /// The diff of `left` against `right` (`compute`'s two sides), and
    /// whether it was just recomputed -- when anything derived from it,
    /// like row highlights, is stale too.
    pub fn get(&mut self, left: &Editor, right: &Editor) -> (&(DiffLines, DiffLines), bool) {
        let revisions = (left.revision(), right.revision());
        let fresh = self.revisions != Some(revisions) || self.diff.is_none();
        if fresh {
            self.revisions = Some(revisions);
            self.diff = Some(compute(&left.text(), &right.text()));
        }
        let diff = self.diff.get_or_insert_with(|| compute(&left.text(), &right.text()));
        (diff, fresh)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{key, unique_scratch_dir};

    fn editor(text: &str, name: &str) -> Editor {
        let path = unique_scratch_dir("diff-cache").join(name);
        std::fs::write(&path, text).unwrap();
        Editor::open(path, None, EditorKeymapMode::Standard).unwrap()
    }

    #[test]
    fn the_diff_is_kept_until_a_text_changes() {
        let (mut left, right) = (editor("a\nb\n", "left.txt"), editor("a\nc\n", "right.txt"));
        let mut cache = DiffCache::default();

        assert!(cache.get(&left, &right).1, "the first time");
        left.input(key(crossterm::event::KeyCode::Down));
        assert!(!cache.get(&left, &right).1, "moving the caret changes nothing");

        left.input(key(crossterm::event::KeyCode::Char('x')));
        let (diff, fresh) = cache.get(&left, &right);
        assert!(fresh, "an edit does");
        assert_eq!(diff.0.kinds.len(), diff.1.kinds.len());
    }

    /// A pane loaded with another file (`Ctrl+L`) is a new editor: its
    /// revision is new too, never one an old editor had.
    #[test]
    fn another_editor_never_reuses_a_revision() {
        let (left, right) = (editor("a\n", "left.txt"), editor("a\n", "right.txt"));
        let mut cache = DiffCache::default();
        cache.get(&left, &right);

        let replaced = editor("z\n", "other.txt");
        assert!(cache.get(&replaced, &right).1);
    }
}
