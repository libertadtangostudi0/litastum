use std::time::SystemTime;

/// A single entry (file or directory) shown in a panel's file list.
///
/// `size`/`modified` aren't read anywhere yet — read from disk up
/// front (`entry_from_dir_entry`) for the panel footer (item
/// count/free space) and a future sort-by-date/size, neither built
/// yet (see `TODO/next-up.md`). `#[allow(dead_code)]` documents
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

pub(crate) fn is_vcs_dir_name(name: &str) -> bool {
    matches!(name, ".git" | ".svn" | ".hg" | ".bzr")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
