//! A tree copied by several threads at once, as `robocopy /MT` does: one
//! walks it, making the directories, and `WORKERS` copy the files. A small
//! file costs far more to open, create and scan than to copy -- measured on
//! 10k files of 4 KB: 4.9 s on one thread, 1.7 s on eight (reported: copying
//! felt very slow). Large files go one at a time, so a hard disk doesn't
//! seek between them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, PoisonError};

use super::copy_file::{self, cancelled};
use super::Progress;

const WORKERS: usize = 8;

/// From this size a file waits for the large one before it.
const LARGE_FILE_BYTES: u64 = 16 << 20;


/// A file for a worker: from, to, its size.
type FileToCopy = (PathBuf, PathBuf, u64);


/// Copies `src` -- a file, or a directory with all in it -- to `dst`. The
/// first failure stops the rest and is returned; `cancel` stops it with
/// `Interrupted`.
pub(super) fn copy_tree(src: &Path, dst: &Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let metadata = fs::symlink_metadata(src)?;
    if !metadata.is_dir() {
        copy_file::copy(src, dst, progress, cancel)?;
        progress.done_items.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }

    let failed = Failure::default();
    let large = Mutex::new(());
    let (sender, receiver) = mpsc::sync_channel::<FileToCopy>(WORKERS * 64);
    let receiver = Mutex::new(receiver);
    let walked = std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| copy_files(&receiver, &large, progress, cancel, &failed));
        }
        let walked = walk(src, dst, &sender, progress, cancel, &failed.stop);
        // Ends the workers once they've taken what's queued.
        drop(sender);
        walked
    });
    match failed.first.into_inner().unwrap_or_else(PoisonError::into_inner) {
        Some(err) => Err(err),
        None => walked,
    }
}


/// The first error a worker met; `stop` tells the others and the walk.
#[derive(Default)]
struct Failure {
    first: Mutex<Option<io::Error>>,
    stop: AtomicBool,
}


/// Makes `dst` and the directories under it, handing every file to the
/// workers. Stops quietly once a worker failed.
fn walk(src: &Path, dst: &Path, files: &SyncSender<FileToCopy>, progress: &Progress, cancel: &AtomicBool, stop: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    progress.done_items.fetch_add(1, Ordering::Relaxed);
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        // From the listing: free on Windows (`work`'s module docs).
        let metadata = entry.metadata()?;
        let to = dst.join(entry.file_name());
        if metadata.is_dir() {
            walk(&entry.path(), &to, files, progress, cancel, stop)?;
        } else if files.send((entry.path(), to, metadata.len())).is_err() {
            return Ok(());
        }
    }
    Ok(())
}


/// A worker: copies files off the queue until it's closed; after a
/// failure or `cancel` it only empties it, so the walk never blocks.
fn copy_files(files: &Mutex<Receiver<FileToCopy>>, large: &Mutex<()>, progress: &Progress, cancel: &AtomicBool, failed: &Failure) {
    loop {
        let next = files.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok((from, to, len)) = next else {
            return;
        };
        if failed.stop.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed) {
            continue;
        }
        let result = if len >= LARGE_FILE_BYTES {
            let _one_at_a_time = large.lock().unwrap_or_else(PoisonError::into_inner);
            copy_file::copy(&from, &to, progress, cancel)
        } else {
            copy_file::copy(&from, &to, progress, cancel)
        };
        match result {
            Ok(()) => {
                progress.done_items.fetch_add(1, Ordering::Relaxed);
            }
            Err(err) => {
                failed.first.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert(err);
                failed.stop.store(true, Ordering::Relaxed);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn tree() -> PathBuf {
        let dir = unique_scratch_dir("copy-tree").join("src");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "12345").unwrap();
        fs::write(dir.join("sub").join("b.txt"), "123").unwrap();
        dir
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

    /// The workers share the files out: every one arrives, whole, however
    /// many more there are than workers.
    #[test]
    fn many_files_all_arrive_whole() {
        let dir = unique_scratch_dir("copy-tree").join("src");
        for d in 0..5 {
            fs::create_dir_all(dir.join(format!("d{d}")).join("deeper")).unwrap();
            for f in 0..40 {
                fs::write(dir.join(format!("d{d}")).join(format!("f{f}.txt")), format!("{d}-{f}")).unwrap();
            }
        }
        let dst = dir.with_file_name("dst");
        let progress = Progress::default();

        copy_tree(&dir, &dst, &progress, &AtomicBool::new(false)).unwrap();

        for d in 0..5 {
            assert!(dst.join(format!("d{d}")).join("deeper").is_dir(), "an empty directory is made too");
            for f in 0..40 {
                assert_eq!(fs::read_to_string(dst.join(format!("d{d}")).join(format!("f{f}.txt"))).unwrap(), format!("{d}-{f}"));
            }
        }
        assert_eq!(progress.done_items.load(Ordering::Relaxed), 1 + 5 * 2 + 5 * 40);
    }

    #[test]
    fn a_file_that_fails_is_reported() {
        let dir = tree();
        let dst = dir.with_file_name("dst");
        // A directory where the file would go: the copy can't create it.
        fs::create_dir_all(dst.join("a.txt")).unwrap();

        assert!(copy_tree(&dir, &dst, &Progress::default(), &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn a_stopped_copy_stops_and_says_so() {
        let dir = tree();
        let dst = dir.with_file_name("dst");

        let err = copy_tree(&dir, &dst, &Progress::default(), &AtomicBool::new(true)).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!dst.exists(), "nothing started");
    }
}
