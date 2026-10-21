//! The tree walks a job does on its thread: counting what's there,
//! copying, deleting for good -- each checks `cancel` between files.
//! Links aren't followed: a link to a directory is copied or deleted as
//! the link, never its target's contents.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::copy_file::{self, cancelled};
use super::Progress;


/// Adds what's under `path` to the totals: its bytes and its entries
/// (files, links and directories alike). Unreadable parts are skipped --
/// they fail later, where the error is reported.
pub(super) fn count(path: &Path, progress: &Progress, cancel: &AtomicBool) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    progress.total_items.fetch_add(1, Ordering::Relaxed);
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                count(&entry.path(), progress, cancel);
            }
        }
    } else {
        progress.total_bytes.fetch_add(metadata.len(), Ordering::Relaxed);
    }
}


/// Copies `src` -- a file, or a directory with all in it -- to `dst`.
pub(super) fn copy_tree(src: &Path, dst: &Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let metadata = fs::symlink_metadata(src)?;
    if metadata.is_dir() {
        fs::create_dir_all(dst)?;
        progress.done_items.fetch_add(1, Ordering::Relaxed);
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()), progress, cancel)?;
        }
        Ok(())
    } else {
        copy_file::copy(src, dst, progress, cancel)?;
        progress.done_items.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}


/// Deletes `path` for good -- a directory with all in it, its files first.
pub(super) fn delete_tree(path: &Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            delete_tree(&entry?.path(), progress, cancel)?;
        }
        fs::remove_dir(path)?;
    } else {
        remove_link_or_file(path, &metadata)?;
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

    #[test]
    fn a_copy_counts_its_bytes_and_entries_as_it_goes() {
        let dir = tree();
        let dst = dir.with_file_name("dst");
        let progress = Progress::default();

        copy_tree(&dir, &dst, &progress, &AtomicBool::new(false)).unwrap();

        assert_eq!(fs::read_to_string(dst.join("sub").join("b.txt")).unwrap(), "123");
        assert_eq!(progress.done_bytes.load(Ordering::Relaxed), 8);
        assert_eq!(progress.done_items.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn a_stopped_copy_stops_and_says_so() {
        let dir = tree();
        let dst = dir.with_file_name("dst");

        let err = copy_tree(&dir, &dst, &Progress::default(), &AtomicBool::new(true)).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dst.exists(), "nothing started");
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
