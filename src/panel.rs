use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;


/// A single entry (file or directory) shown in a panel's file list.
///
/// `size`/`modified` aren't read anywhere yet — read from disk up
/// front (`entry_from_dir_entry`) for the panel footer (item
/// count/free space) and a future sort-by-date/size, neither built
/// yet (see `TODO.md`'s "Next up"). `#[allow(dead_code)]` documents
/// that as deliberate instead of silencing a real oversight.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}


/// A coarse category `ui.rs` colors entries by — Far Manager-style
/// file highlighting, kept deliberately small (a handful of common
/// extension groups, not an attempt at Far's own regex-based
/// `highlighting.hgh` rule system). Reverses an earlier, explicit
/// "directories are distinguished only by a trailing `/`, no file-type
/// color dots" decision (`.claude/rules/litastum-ui-theme.md`) — kept
/// per an explicit later request rather than silently dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightRole {
    /// `..`, styled apart from real entries.
    Parent,
    /// A directory with no special meaning — styled the same as
    /// `Other`, per an actual Far Manager screenshot checked while
    /// building this: ordinary directories (`.cargo`, `src`, `target`,
    /// ...) render in plain text, same as files. Only version-control
    /// metadata directories stand out (`VcsDirectory`, below) — an
    /// earlier version of this colored *every* directory, which wasn't
    /// what the reference actually showed.
    Directory,
    /// A VCS metadata directory (`.git`, `.svn`, `.hg`, `.bzr`) — the
    /// one directory case Far's own reference screenshot did color
    /// distinctly.
    VcsDirectory,
    Archive,
    /// Executables and script files.
    Executable,
    Other,
}


impl Entry {
    /// Classifies this entry for `ui.rs`'s coloring. Extension/name
    /// lists are intentionally short — common cases, not exhaustive.
    pub fn highlight_role(&self) -> HighlightRole {
        if self.name == ".." {
            return HighlightRole::Parent;
        }
        if self.is_dir {
            return if is_vcs_dir_name(&self.name) {
                HighlightRole::VcsDirectory
            } else {
                HighlightRole::Directory
            };
        }

        match self.extension().as_deref() {
            Some("zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz") => HighlightRole::Archive,
            Some("exe" | "bat" | "cmd" | "sh" | "ps1" | "py" | "js" | "ts" | "rb" | "pl") => {
                HighlightRole::Executable
            }
            _ => HighlightRole::Other,
        }
    }


    /// Lowercased file extension, if any (`"Foo.PY"` → `Some("py")`).
    fn extension(&self) -> Option<String> {
        std::path::Path::new(&self.name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
    }
}


fn is_vcs_dir_name(name: &str) -> bool {
    matches!(name, ".git" | ".svn" | ".hg" | ".bzr")
}


/// One of the two file-list panes. Holds its current directory, the
/// entries within it, and which entry the cursor is on.
///
/// The entry list is displayed column-major (fill column 1 top to
/// bottom, then column 2, ...) rather than as a single flat list, so
/// `entries[i]`'s column is `i / rows()` and its row is `i % rows()`.
/// `columns` is written by the renderer each frame from the panel's
/// on-screen width — see `ui::draw`.
#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub columns: usize,
}


impl Panel {
    /// Creates a panel rooted at `path` and immediately loads its contents.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let mut panel = Self {
            path,
            entries: Vec::new(),
            selected: 0,
            columns: 1,
        };
        panel.reload()?;
        Ok(panel)
    }


    /// Re-reads the current directory from disk, replacing `entries`.
    /// A synthetic ".." entry is inserted first when the directory has
    /// a parent, so the cursor can always navigate upward.
    pub fn reload(&mut self) -> io::Result<()> {
        let mut entries = Vec::new();

        if self.path.parent().is_some() {
            entries.push(Entry {
                name: "..".to_string(),
                is_dir: true,
                size: 0,
                modified: None,
            });
        }

        let mut children: Vec<Entry> = fs::read_dir(&self.path)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| Self::entry_from_dir_entry(&entry).ok())
            .collect();

        children.sort_by(Self::compare_entries);
        entries.extend(children);

        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        Ok(())
    }


    fn entry_from_dir_entry(entry: &fs::DirEntry) -> io::Result<Entry> {
        let metadata = entry.metadata()?;
        Ok(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }


    /// Directories sort before files; within each group, names sort
    /// case-insensitively.
    fn compare_entries(a: &Entry, b: &Entry) -> Ordering {
        match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    }


    /// The entry currently under the cursor, if the panel is non-empty.
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }


    /// Number of display rows for the current column count. Zero when
    /// there are no entries or no columns have been assigned yet.
    fn rows(&self) -> usize {
        if self.columns == 0 {
            return 0;
        }
        self.entries.len().div_ceil(self.columns)
    }


    /// Sets the display column count — recomputed by the renderer each
    /// frame from the panel's on-screen width — and re-clamps the
    /// cursor in case a resize shrank the entry it pointed at.
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }


    /// Moves the cursor up one row. At the top of a column this flows
    /// into the bottom of the previous column — `entries` is already
    /// stored in column-major order, so a plain linear step does this
    /// correctly on its own. Clamped at the very first entry.
    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    /// Moves the cursor down one row. At the bottom of a column this
    /// flows into the top of the next column, for the same reason as
    /// `move_up`. Clamped at the very last entry.
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }


    /// Moves the cursor one column left, keeping the same row, clamped
    /// at the first column.
    pub fn move_left(&mut self) {
        let rows = self.rows();
        if rows > 0 {
            self.selected = self.selected.saturating_sub(rows);
        }
    }


    /// Moves the cursor one column right, keeping the same row,
    /// clamped at the last column.
    pub fn move_right(&mut self) {
        let rows = self.rows();
        if rows == 0 {
            return;
        }
        let next = self.selected + rows;
        if next < self.entries.len() {
            self.selected = next;
        }
    }


    /// Enters the directory under the cursor (or its parent, for "..").
    /// Does nothing if the current entry is a file.
    pub fn enter_selected(&mut self) -> io::Result<()> {
        let Some(entry) = self.current() else {
            return Ok(());
        };
        if !entry.is_dir {
            return Ok(());
        }

        let new_path = if entry.name == ".." {
            match self.path.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return Ok(()),
            }
        } else {
            self.path.join(&entry.name)
        };

        self.path = new_path;
        self.selected = 0;
        self.reload()
    }


    /// Full path to the entry currently under the cursor, if any.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.current().map(|entry| self.path.join(&entry.name))
    }


    /// Changes to `target`, resolved relative to the current path (an
    /// absolute `target` replaces it outright — `Path::join`'s own
    /// behavior, no separate case needed) and lexically normalized
    /// (`..` segments collapsed, so `cd ..` leaves a clean parent path
    /// rather than `.../sub/..` — found by a test that (correctly)
    /// expected the clean form). Used by the command line's `cd`
    /// handling (`command_line::parse_cd_target`); a target that
    /// doesn't resolve to a real directory is silently ignored rather
    /// than erroring, matching a real shell's tolerance for a typo'd
    /// `cd` not crashing anything.
    pub fn change_dir(&mut self, target: &str) -> io::Result<()> {
        let new_path = lexically_normalize(&self.path.join(target));
        if !new_path.is_dir() {
            return Ok(());
        }
        self.path = new_path;
        self.selected = 0;
        self.reload()
    }
}


/// Collapses `.`/`..` path components without touching the filesystem
/// (no symlink resolution, unlike `fs::canonicalize` — which also
/// prepends the `\\?\` extended-length prefix on Windows, an ugly,
/// separate annoyance not worth taking on just to normalize `..`).
fn lexically_normalize(path: &std::path::Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use super::*;

    /// A real scratch directory with one real subdirectory (`sub`) in
    /// it, for `change_dir` tests — unlike `panel_with` below, this
    /// needs actual filesystem entries since `change_dir` checks
    /// `is_dir()` and calls `reload()`. Distinct per test (`cargo
    /// test` runs in parallel threads within one process).
    fn scratch_panel() -> Panel {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litastum-panel-test-{}-{n}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).expect("create scratch dirs");
        Panel::new(dir).expect("open scratch panel")
    }

    #[test]
    fn change_dir_descends_into_a_real_subdirectory() {
        let mut panel = scratch_panel();
        let original = panel.path.clone();

        panel.change_dir("sub").unwrap();

        assert_eq!(panel.path, original.join("sub"));
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn change_dir_ignores_a_target_that_is_not_a_directory() {
        let mut panel = scratch_panel();
        let original = panel.path.clone();

        panel.change_dir("does-not-exist").unwrap();

        assert_eq!(panel.path, original, "path should be unchanged");
    }

    #[test]
    fn change_dir_up_via_dotdot() {
        let mut panel = scratch_panel();
        let original = panel.path.clone();
        panel.change_dir("sub").unwrap();

        panel.change_dir("..").unwrap();

        assert_eq!(panel.path, original);
    }

    #[test]
    fn lexically_normalize_collapses_parent_dir_segments() {
        assert_eq!(lexically_normalize(std::path::Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(lexically_normalize(std::path::Path::new("/a/b/..")), PathBuf::from("/a"));
        assert_eq!(lexically_normalize(std::path::Path::new("/a/./b")), PathBuf::from("/a/b"));
    }

    /// A panel with `count` dummy file entries, laid out in `columns`
    /// columns, cursor starting at index 0.
    fn panel_with(count: usize, columns: usize) -> Panel {
        let entries = (0..count)
            .map(|i| Entry {
                name: i.to_string(),
                is_dir: false,
                size: 0,
                modified: None,
            })
            .collect();
        Panel {
            path: PathBuf::new(),
            entries,
            selected: 0,
            columns,
        }
    }

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry { name: name.to_string(), is_dir, size: 0, modified: None }
    }

    #[test]
    fn parent_entry_is_its_own_role_even_though_its_marked_as_a_dir() {
        assert_eq!(entry("..", true).highlight_role(), HighlightRole::Parent);
    }

    #[test]
    fn ordinary_directories_are_not_specially_colored() {
        assert_eq!(entry("src", true).highlight_role(), HighlightRole::Directory);
        assert_eq!(entry("archive.zip", true).highlight_role(), HighlightRole::Directory, "a dir named like an archive is still a dir, not Archive");
    }

    #[test]
    fn vcs_metadata_directories_are_their_own_role() {
        for name in [".git", ".svn", ".hg", ".bzr"] {
            assert_eq!(entry(name, true).highlight_role(), HighlightRole::VcsDirectory, "{name}");
        }
    }

    #[test]
    fn a_file_named_like_a_vcs_dir_is_not_treated_as_one() {
        assert_eq!(entry(".git", false).highlight_role(), HighlightRole::Other);
    }

    #[test]
    fn archives_are_classified_by_extension() {
        assert_eq!(entry("backup.zip", false).highlight_role(), HighlightRole::Archive);
        assert_eq!(entry("data.TAR", false).highlight_role(), HighlightRole::Archive, "extension match is case-insensitive");
    }

    #[test]
    fn scripts_and_executables_are_classified_by_extension() {
        assert_eq!(entry("run.py", false).highlight_role(), HighlightRole::Executable);
        assert_eq!(entry("build.SH", false).highlight_role(), HighlightRole::Executable);
    }

    #[test]
    fn everything_else_is_other() {
        assert_eq!(entry("README.md", false).highlight_role(), HighlightRole::Other);
        assert_eq!(entry("no_extension", false).highlight_role(), HighlightRole::Other);
    }

    // 5 entries, 2 columns -> rows = 3: col0 = [0,1,2], col1 = [3,4]
    // (col1's row 2 doesn't exist — 5 doesn't divide evenly by 2).

    #[test]
    fn move_down_flows_into_next_column_at_column_bottom() {
        let mut panel = panel_with(5, 2);
        panel.move_down();
        panel.move_down();
        assert_eq!(panel.selected, 2, "still col0, its last row");
        panel.move_down();
        assert_eq!(panel.selected, 3, "flowed into col1, row0");
    }

    #[test]
    fn move_down_clamped_at_last_entry() {
        let mut panel = panel_with(5, 2);
        panel.selected = 4;
        panel.move_down();
        assert_eq!(panel.selected, 4);
    }

    #[test]
    fn move_up_flows_into_previous_column_at_column_top() {
        let mut panel = panel_with(5, 2);
        panel.selected = 3; // col1, row0
        panel.move_up();
        assert_eq!(panel.selected, 2, "flowed into col0, its last row");
    }

    #[test]
    fn move_up_clamped_at_first_entry() {
        let mut panel = panel_with(5, 2);
        panel.move_up();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn move_right_lands_in_next_column_same_row() {
        let mut panel = panel_with(5, 2);
        panel.selected = 1; // col0, row1
        panel.move_right();
        assert_eq!(panel.selected, 4); // col1, row1
    }

    #[test]
    fn move_right_clamped_when_last_column_is_shorter() {
        let mut panel = panel_with(5, 2);
        panel.selected = 2; // col0, row2 -- col1 has no row2
        panel.move_right();
        assert_eq!(panel.selected, 2);
    }

    #[test]
    fn move_left_clamped_at_first_column() {
        let mut panel = panel_with(5, 2);
        panel.move_left();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn navigation_on_empty_panel_is_noop() {
        let mut panel = panel_with(0, 2);
        panel.move_up();
        panel.move_down();
        panel.move_left();
        panel.move_right();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn set_columns_reclamps_selected_after_shrinking() {
        let mut panel = panel_with(5, 2);
        panel.selected = 4;
        panel.entries.truncate(2);
        panel.set_columns(1);
        assert_eq!(panel.selected, 1);
    }
}
