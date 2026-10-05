use std::path::PathBuf;

/// The four files SVN leaves on a merge conflict: the file itself, now
/// holding conflict markers, and three sidecars named after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFiles {
    /// `X`: the file with conflict markers -- the one being resolved.
    pub result: PathBuf,
    /// `X.working`: my version before the merge.
    pub working: PathBuf,
    /// `X.merge-left.rN`: what the incoming change started from.
    pub base: PathBuf,
    /// `X.merge-right.rN`: the incoming version.
    pub theirs: PathBuf,
}


/// Recognizes exactly those four paths, in any order: `X`, `X.working`,
/// `X.merge-left.r<N>` and `X.merge-right.r<N>`, all in one directory.
/// Anything else is `None`, so `Alt+F5` falls back to plain Compare.
pub fn detect(paths: &[PathBuf]) -> Option<ConflictFiles> {
    if paths.len() != 4 {
        return None;
    }
    paths.iter().find_map(|result| {
        let name = result.file_name()?.to_str()?;
        let dir = result.parent()?;
        let (mut working, mut base, mut theirs) = (None, None, None);
        for other in paths.iter().filter(|other| *other != result) {
            if other.parent()? != dir {
                return None;
            }
            let suffix = other.file_name()?.to_str()?.strip_prefix(name)?;
            if suffix == ".working" {
                working = Some(other.clone());
            } else if is_revision_suffix(suffix, ".merge-left.r") {
                base = Some(other.clone());
            } else if is_revision_suffix(suffix, ".merge-right.r") {
                theirs = Some(other.clone());
            } else {
                return None;
            }
        }
        Some(ConflictFiles { result: result.clone(), working: working?, base: base?, theirs: theirs? })
    })
}


fn is_revision_suffix(suffix: &str, prefix: &str) -> bool {
    suffix.strip_prefix(prefix).is_some_and(|revision| !revision.is_empty() && revision.bytes().all(|byte| byte.is_ascii_digit()))
}


#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn paths(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|name| dir.join(name)).collect()
    }

    #[test]
    fn recognizes_the_four_files_in_any_order() {
        let dir = Path::new("src");
        let found = detect(&paths(dir, &["a.h.merge-right.r175688", "a.h", "a.h.working", "a.h.merge-left.r175687"])).unwrap();
        assert_eq!(
            found,
            ConflictFiles {
                result: dir.join("a.h"),
                working: dir.join("a.h.working"),
                base: dir.join("a.h.merge-left.r175687"),
                theirs: dir.join("a.h.merge-right.r175688"),
            }
        );
    }

    #[test]
    fn needs_exactly_four_files() {
        let dir = Path::new("src");
        assert_eq!(detect(&paths(dir, &["a.h", "a.h.working", "a.h.merge-left.r1"])), None);
        assert_eq!(detect(&paths(dir, &["a.h", "a.h.working", "a.h.merge-left.r1", "a.h.merge-right.r2", "b.h"])), None);
    }

    #[test]
    fn rejects_a_missing_role_or_a_foreign_name() {
        let dir = Path::new("src");
        assert_eq!(detect(&paths(dir, &["a.h", "a.h.working", "a.h.merge-left.r1", "a.h.merge-left.r2"])), None, "no merge-right");
        assert_eq!(detect(&paths(dir, &["a.h", "a.h.working", "a.h.merge-left.r1", "b.h.merge-right.r2"])), None);
        assert_eq!(detect(&paths(dir, &["a.h", "a.h.working", "a.h.merge-left.rX", "a.h.merge-right.r2"])), None, "revision isn't a number");
        assert_eq!(detect(&paths(dir, &["a.h", "a.h.working", "a.h.merge-left.r", "a.h.merge-right.r2"])), None, "no revision");
    }

    #[test]
    fn rejects_files_from_different_directories() {
        let mut files = paths(Path::new("src"), &["a.h", "a.h.working", "a.h.merge-left.r1"]);
        files.push(Path::new("docs").join("a.h.merge-right.r2"));
        assert_eq!(detect(&files), None);
    }
}
