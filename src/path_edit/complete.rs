use std::fs;
use std::path::Path;

/// The `Tab` list under a path field: the entries of the typed path's
/// directory whose names start with its last part. Directories first.
/// May be empty -- it then says so, and stays open while typing, so
/// deleting a wrong character brings the matches back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completions {
    /// The typed text up to and including its last separator, put back in
    /// front of the accepted name.
    dir_part: String,
    /// Name and whether it's a directory.
    pub items: Vec<(String, bool)>,
    pub selected: usize,
}


/// Every entry of `dir` whose name starts with `prefix`, ignoring case:
/// name and whether it's a directory, unsorted. Empty if `dir` can't be
/// read. Shared with the command line's own `Tab`.
pub(crate) fn matching_entries(dir: &Path, prefix: &str) -> Vec<(String, bool)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let prefix = prefix.to_lowercase();
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.to_lowercase().starts_with(&prefix).then(|| (name, entry.file_type().is_ok_and(|kind| kind.is_dir())))
        })
        .collect()
}


/// What `Tab` does to `text`, a path relative to `base` unless absolute.
#[derive(Debug, PartialEq, Eq)]
pub enum Tab {
    /// One match, written out in full (a directory with a trailing
    /// separator, so the next `Tab` goes inside it).
    Complete(String),
    /// Several, or none: the text extended to their shared start, and the
    /// list (empty when nothing matches).
    Choose(String, Completions),
}


pub fn tab(text: &str, base: &Path) -> Tab {
    let mut completions = Completions::list(text, base);
    if let [only] = completions.items.as_slice() {
        return Tab::Complete(completions.text_for(only));
    }
    if completions.items.is_empty() {
        return Tab::Choose(text.to_string(), completions);
    }
    let shared = shared_start(completions.items.iter().map(|(name, _)| name.as_str()));
    let typed = &text[completions.dir_part.len()..];
    let extended = if shared.chars().count() > typed.chars().count() { format!("{}{shared}", completions.dir_part) } else { text.to_string() };
    completions.selected = 0;
    Tab::Choose(extended, completions)
}


impl Completions {
    /// The matches for `text`, possibly none.
    pub fn list(text: &str, base: &Path) -> Self {
        let dir_end = text.rfind(['/', '\\']).map_or(0, |i| i + 1);
        let (dir_part, prefix) = text.split_at(dir_end);
        let mut items = matching_entries(&base.join(dir_part), prefix);
        items.sort_by(|(a, a_dir), (b, b_dir)| b_dir.cmp(a_dir).then_with(|| a.to_lowercase().cmp(&b.to_lowercase())));
        Self { dir_part: dir_part.to_string(), items, selected: 0 }
    }

    /// The field's text with the highlighted entry accepted; `None` when
    /// nothing matches.
    pub fn accepted(&self) -> Option<String> {
        self.items.get(self.selected).map(|item| self.text_for(item))
    }

    fn text_for(&self, (name, is_dir): &(String, bool)) -> String {
        let separator = if *is_dir { std::path::MAIN_SEPARATOR_STR } else { "" };
        format!("{}{name}{separator}", self.dir_part)
    }

    pub fn up(&mut self) {
        crate::list_cursor::move_up(&mut self.selected);
    }

    pub fn down(&mut self) {
        crate::list_cursor::move_down(&mut self.selected, self.items.len());
    }
}


/// The longest start every name shares, ignoring case; spelled as the
/// first name spells it.
fn shared_start<'a>(mut names: impl Iterator<Item = &'a str>) -> String {
    let Some(first) = names.next() else {
        return String::new();
    };
    let mut length = first.chars().count();
    for name in names {
        length = first.chars().zip(name.chars()).take(length).take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase())).count();
    }
    first.chars().take(length).collect()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    const SEP: &str = std::path::MAIN_SEPARATOR_STR;

    fn tree() -> std::path::PathBuf {
        let dir = unique_scratch_dir("path-complete");
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::create_dir_all(dir.join("TODO")).unwrap();
        fs::write(dir.join("todo.txt"), "").unwrap();
        fs::write(dir.join("Cargo.toml"), "").unwrap();
        dir
    }

    #[test]
    fn one_match_is_written_out_with_a_separator_for_a_directory() {
        let dir = tree();
        assert_eq!(tab("tar", &dir), Tab::Complete(format!("target{SEP}")));
        assert_eq!(tab("car", &dir), Tab::Complete("Cargo.toml".into()), "case doesn't matter");
    }

    #[test]
    fn several_matches_extend_to_their_shared_start_and_list_them_directories_first() {
        let dir = tree();
        let Tab::Choose(text, list) = tab("t", &dir) else { panic!("expected a list") };
        assert_eq!(text, "t", "target/TODO/todo.txt share only the t");
        let names: Vec<&str> = list.items.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["target", "TODO", "todo.txt"]);

        let Tab::Choose(text, _) = tab("to", &dir) else { panic!("expected a list") };
        assert_eq!(text, "TODO", "TODO and todo.txt share 'todo', spelled as the first");
    }

    #[test]
    fn an_absolute_path_completes_from_its_own_directory() {
        let dir = tree();
        let typed = format!("{}{SEP}ta", dir.display());
        assert_eq!(tab(&typed, Path::new("elsewhere")), Tab::Complete(format!("{}{SEP}target{SEP}", dir.display())));
    }

    #[test]
    fn accepting_from_the_list_and_nothing_to_complete() {
        let dir = tree();
        let Tab::Choose(_, mut list) = tab("t", &dir) else { panic!("expected a list") };
        list.down();
        assert_eq!(list.accepted(), Some(format!("TODO{SEP}")));
        let Tab::Choose(text, none) = tab("zzz", &dir) else { panic!("expected an empty list") };
        assert_eq!((text.as_str(), none.items.len(), none.accepted()), ("zzz", 0, None), "the text stays, the list says so");
        assert!(matches!(tab("no/such/", &dir), Tab::Choose(_, list) if list.items.is_empty()));
    }
}
