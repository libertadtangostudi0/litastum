//! How a save writes the file (requested: saving made safe): over the
//! file that's there, not a new one in its place -- so its attributes,
//! permissions, hard links and NTFS streams stay -- and flushed to the
//! disk before the save counts as done. `fs::write` also wrote over the
//! file, but cut it to nothing first (a failure part-way left it empty)
//! and didn't flush (after a power cut the save could be lost).
//! History: docs/history/editor-saving.md.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::SystemTime;


/// What a file on disk looked like when it was read or last saved: a
/// save checks it's still so (`Editor::save_checked`) -- another program,
/// `hg pull`, `git checkout` may have changed it meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DiskStamp {
    modified: Option<SystemTime>,
    len: u64,
}


/// `path`'s stamp now; `None` if it isn't there.
pub(super) fn stamp(path: &Path) -> Option<DiskStamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(DiskStamp { modified: metadata.modified().ok(), len: metadata.len() })
}


/// Writes `contents` to `path`, over the file if there is one, and waits
/// for it to reach the disk. The new text goes first and the old tail is
/// cut after it: a failure part-way leaves the start written, never an
/// emptied file.
pub(super) fn write_in_place(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = match OpenOptions::new().write(true).open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => File::create(path)?,
        Err(err) => return Err(err),
    };
    file.seek(SeekFrom::Start(0))?;
    file.write_all(contents)?;
    file.set_len(contents.len() as u64)?;
    file.sync_all()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    #[test]
    fn a_shorter_text_leaves_no_old_tail_and_a_new_file_is_made() {
        let dir = unique_scratch_dir("file-write");
        let path = dir.join("a.txt");
        std::fs::write(&path, "a much longer old text").unwrap();

        write_in_place(&path, b"short").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "short");

        let new = dir.join("new.txt");
        write_in_place(&new, b"made").unwrap();
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "made");
    }

    /// The same file, written over: a hard link to it sees the new text
    /// (a new file renamed in would cut the link).
    #[test]
    fn a_hard_link_keeps_seeing_the_file() {
        let dir = unique_scratch_dir("file-write");
        let path = dir.join("a.txt");
        let link = dir.join("link.txt");
        std::fs::write(&path, "old").unwrap();
        std::fs::hard_link(&path, &link).unwrap();

        write_in_place(&path, b"new").unwrap();

        assert_eq!(std::fs::read_to_string(&link).unwrap(), "new");
    }

    /// Written over, not replaced: a hidden file stays hidden and keeps its
    /// other NTFS streams.
    #[cfg(windows)]
    #[test]
    fn a_hidden_file_stays_hidden_and_keeps_its_streams() {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};

        let path = unique_scratch_dir("file-write").join("hidden.txt");
        std::fs::write(&path, "old").unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated path we own.
        assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) }, 0);
        let stream = format!("{}:extra", path.display());
        std::fs::write(&stream, "stream").unwrap();

        write_in_place(&path, b"new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_ne!(std::fs::metadata(&path).unwrap().file_attributes() & FILE_ATTRIBUTE_HIDDEN, 0, "still hidden");
        assert_eq!(std::fs::read_to_string(&stream).unwrap(), "stream", "the other stream is kept");
    }
}
