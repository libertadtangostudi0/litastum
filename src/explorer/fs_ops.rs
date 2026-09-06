use std::fs;
use std::io;
use std::path::Path;


/// Copies `src` to `dst` — a plain `fs::copy` for a file, or a full
/// recursive tree copy for a directory (`std::fs` has no built-in for
/// that, unlike `remove_dir_all`). Used by F5 (`Mode::ConfirmTransfer`
/// with `TransferOp::Copy`).
pub fn copy_entry(src: &Path, dst: &Path, is_dir: bool) -> io::Result<()> {
    if is_dir {
        copy_dir_recursive(src, dst)
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}


fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}


/// Moves `src` to `dst` — tries `fs::rename` first (instant, the common
/// case within one drive/filesystem), and falls back to copy-then-
/// delete only if that fails (e.g. `rename` across drives on Windows,
/// which always errors rather than transparently copying like Unix
/// `rename(2)` sometimes does across bind mounts). Used by F6
/// (`Mode::ConfirmTransfer` with `TransferOp::Move`).
pub fn move_entry(src: &Path, dst: &Path, is_dir: bool) -> io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }

    copy_entry(src, dst, is_dir)?;
    if is_dir {
        fs::remove_dir_all(src)
    } else {
        fs::remove_file(src)
    }
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A fresh scratch directory under the OS temp dir, unique per test
    /// (`cargo test` runs in parallel threads within one process — same
    /// pattern as `panel.rs::scratch_panel`).
    fn scratch_dir() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litastum-fs-ops-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn copy_entry_copies_a_single_file() {
        let dir = scratch_dir();
        let src = dir.join("source.txt");
        fs::write(&src, b"hello").unwrap();
        let dst = dir.join("dest.txt");

        copy_entry(&src, &dst, false).unwrap();

        assert!(src.exists(), "source should still exist after a copy");
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn copy_entry_recursively_copies_a_directory() {
        let dir = scratch_dir();
        let src = dir.join("src_tree");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("top.txt"), b"top").unwrap();
        fs::write(src.join("nested").join("deep.txt"), b"deep").unwrap();
        let dst = dir.join("dst_tree");

        copy_entry(&src, &dst, true).unwrap();

        assert_eq!(fs::read(dst.join("top.txt")).unwrap(), b"top");
        assert_eq!(fs::read(dst.join("nested").join("deep.txt")).unwrap(), b"deep");
        assert!(src.join("top.txt").exists(), "source tree should be untouched");
    }

    #[test]
    fn move_entry_moves_a_file() {
        let dir = scratch_dir();
        let src = dir.join("source.txt");
        fs::write(&src, b"hello").unwrap();
        let dst = dir.join("dest.txt");

        move_entry(&src, &dst, false).unwrap();

        assert!(!src.exists(), "source should be gone after a move");
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn move_entry_moves_a_directory() {
        let dir = scratch_dir();
        let src = dir.join("src_tree");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file.txt"), b"content").unwrap();
        let dst = dir.join("dst_tree");

        move_entry(&src, &dst, true).unwrap();

        assert!(!src.exists(), "source tree should be gone after a move");
        assert_eq!(fs::read(dst.join("file.txt")).unwrap(), b"content");
    }
}
