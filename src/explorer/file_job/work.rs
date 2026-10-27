//! The tree walks a job does on its thread: counting what's there,
//! deleting for good -- each checks `cancel` between files (copying is
//! `copy_tree`'s).
//! Links aren't followed: a link to a directory is copied or deleted as
//! the link, never its target's contents. Below the top entry, metadata
//! comes from the directory listing (`DirEntry::metadata`, free on
//! Windows): asking for it per entry made counting 50k files take 3 s,
//! not 0.05 s (reported: counting took very long).

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::copy_file::cancelled;
use super::Progress;


/// Adds what's under `path` to the totals: its bytes and its entries
/// (files, links and directories alike). Unreadable parts are skipped --
/// they fail later, where the error is reported.
pub(super) fn count(path: &Path, progress: &Progress, cancel: &AtomicBool) {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        count_entry(path, &metadata, progress, cancel);
    }
}


fn count_entry(path: &Path, metadata: &fs::Metadata, progress: &Progress, cancel: &AtomicBool) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    progress.total_items.fetch_add(1, Ordering::Relaxed);
    if !metadata.is_dir() {
        progress.total_bytes.fetch_add(metadata.len(), Ordering::Relaxed);
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            count_entry(&entry.path(), &metadata, progress, cancel);
        }
    }
}


/// Whether `path` is `dir` or anything under it -- by their absolute,
/// lexically normalized forms, letter case aside on Windows (`C:\A` and
/// `c:\a\b` are the same tree there).
pub(super) fn is_within(path: &Path, dir: &Path) -> bool {
    let normalized = |path: &Path| -> Option<Vec<String>> {
        let absolute = std::path::absolute(path).ok()?;
        let parts = absolute.components().filter(|part| !matches!(part, std::path::Component::CurDir)).map(|part| part.as_os_str().to_string_lossy().into_owned());
        Some(if cfg!(windows) { parts.map(|part| part.to_lowercase()).collect() } else { parts.collect() })
    };
    match (normalized(path), normalized(dir)) {
        (Some(path), Some(dir)) => path.starts_with(&dir),
        _ => false,
    }
}


/// Deletes `path` for good -- a directory with all in it, its files first.
pub(super) fn delete_tree(path: &Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    delete_entry(path, &fs::symlink_metadata(path)?, progress, cancel)
}


fn delete_entry(path: &Path, metadata: &fs::Metadata, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            delete_entry(&entry.path(), &entry.metadata()?, progress, cancel)?;
        }
        fs::remove_dir(path)?;
    } else {
        remove_link_or_file(path, metadata)?;
    }
    progress.done_items.fetch_add(1, Ordering::Relaxed);
    Ok(())
}


/// A link to a directory is removed as a directory on Windows.
fn remove_link_or_file(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let result = fs::remove_file(path);
    if result.is_err() && metadata.file_type().is_symlink() {
        return fs::remove_dir(path);
    }
    result
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn tree() -> std::path::PathBuf {
        let dir = unique_scratch_dir("file-job-work").join("src");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "12345").unwrap();
        fs::write(dir.join("sub").join("b.txt"), "123").unwrap();
        dir
    }

    #[test]
    fn counting_adds_the_bytes_and_every_entry() {
        let dir = tree();
        let progress = Progress::default();
        count(&dir, &progress, &AtomicBool::new(false));
        assert_eq!(progress.total_bytes.load(Ordering::Relaxed), 8);
        assert_eq!(progress.total_items.load(Ordering::Relaxed), 4, "the directory, sub, two files");
    }

    /// A folder copied into its own subfolder copied itself until the disk
    /// was full; `begin_transfer` refuses what `is_within` finds.
    #[test]
    fn a_path_is_within_its_own_tree_only() {
        let dir = tree();
        assert!(is_within(&dir.join("sub").join("copy"), &dir));
        assert!(is_within(&dir, &dir), "onto itself");
        assert!(!is_within(&dir.with_file_name("src2"), &dir), "a sibling with the same start is not inside");
        assert!(!is_within(&dir.with_file_name("dst"), &dir));
        #[cfg(windows)]
        assert!(is_within(std::path::Path::new(&dir.join("SUB").to_string_lossy().to_uppercase()), &dir), "letter case aside");
    }

    #[test]
    fn delete_counts_each_entry() {
        let dir = tree();
        let progress = Progress::default();

        delete_tree(&dir, &progress, &AtomicBool::new(false)).unwrap();

        assert!(!dir.exists());
        assert_eq!(progress.done_items.load(Ordering::Relaxed), 4);
    }
}
