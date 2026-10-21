//! One file copied with its bytes counted as they go, and stoppable
//! mid-file. On Windows `CopyFileExW`, as `fs::copy` does (attributes and
//! streams kept), with its progress callback; elsewhere a chunked copy.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::Progress;


/// What `copy` says when `cancel` stopped it.
pub(super) fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "stopped")
}


#[cfg(windows)]
pub(super) fn copy(src: &Path, dst: &Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::CopyFileExW;

    struct Context<'a> {
        progress: &'a Progress,
        cancel: &'a AtomicBool,
        counted: u64,
    }

    /// Called by `CopyFileExW` as it goes: adds what was copied since the
    /// last call, and stops it once `cancel` is set (the partial file is
    /// removed by `CopyFileExW` itself).
    unsafe extern "system" fn on_progress(
        _total: i64,
        transferred: i64,
        _stream_size: i64,
        _stream_transferred: i64,
        _stream: u32,
        _reason: u32,
        _source: *mut core::ffi::c_void,
        _destination: *mut core::ffi::c_void,
        data: *const core::ffi::c_void,
    ) -> u32 {
        const PROGRESS_CONTINUE: u32 = 0;
        const PROGRESS_CANCEL: u32 = 1;
        // SAFETY: `data` is the `Context` `copy` passes, alive for the call.
        let context = unsafe { &mut *(data as *mut Context) };
        let transferred = transferred.max(0) as u64;
        context.progress.done_bytes.fetch_add(transferred.saturating_sub(context.counted), Ordering::Relaxed);
        context.counted = transferred;
        if context.cancel.load(Ordering::Relaxed) {
            PROGRESS_CANCEL
        } else {
            PROGRESS_CONTINUE
        }
    }

    let wide = |path: &Path| -> io::Result<Vec<u16>> { Ok(long_path(&std::path::absolute(path)?).encode_wide().chain([0]).collect()) };
    let (from, to) = (wide(src)?, wide(dst)?);
    let mut context = Context { progress, cancel, counted: 0 };
    // SAFETY: both paths are NUL-terminated and, with `context`, outlive
    // the call.
    let ok = unsafe { CopyFileExW(from.as_ptr(), to.as_ptr(), Some(on_progress), &mut context as *mut Context as *const core::ffi::c_void, std::ptr::null_mut(), 0) };
    if ok != 0 {
        return Ok(());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    Err(io::Error::last_os_error())
}


/// `path` (absolute) in its `\\?\` form, so a path past 260 characters
/// works -- `std::fs` adds it itself, a raw `CopyFileExW` call doesn't.
#[cfg(windows)]
fn long_path(path: &Path) -> std::ffi::OsString {
    let text = path.as_os_str().to_string_lossy();
    if text.starts_with(r"\\?\") {
        return path.as_os_str().to_owned();
    }
    match text.strip_prefix(r"\\") {
        Some(unc) => format!(r"\\?\UNC\{unc}").into(),
        None => format!(r"\\?\{text}").into(),
    }
}


#[cfg(not(windows))]
pub(super) fn copy(src: &Path, dst: &Path, progress: &Progress, cancel: &AtomicBool) -> io::Result<()> {
    use std::io::{Read, Write};

    let mut from = std::fs::File::open(src)?;
    let mut to = std::fs::File::create(dst)?;
    let mut buffer = vec![0u8; 1 << 20];
    let result = (|| loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let read = from.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        to.write_all(&buffer[..read])?;
        progress.done_bytes.fetch_add(read as u64, Ordering::Relaxed);
    })();
    match result {
        Ok(()) => to.set_permissions(from.metadata()?.permissions()),
        Err(err) => {
            drop(to);
            let _ = std::fs::remove_file(dst);
            Err(err)
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    #[cfg(windows)]
    #[test]
    fn a_long_path_gets_its_verbatim_prefix() {
        assert_eq!(long_path(Path::new(r"C:\a\b")), std::ffi::OsString::from(r"\\?\C:\a\b"));
        assert_eq!(long_path(Path::new(r"\\server\share\x")), std::ffi::OsString::from(r"\\?\UNC\server\share\x"));
        assert_eq!(long_path(Path::new(r"\\?\C:\a")), std::ffi::OsString::from(r"\\?\C:\a"));
    }

    /// Past 260 characters a raw `CopyFileExW` failed without the prefix.
    #[test]
    fn a_file_deeper_than_260_characters_copies() {
        let mut dir = unique_scratch_dir("long-path");
        while dir.as_os_str().len() < 300 {
            dir = dir.join("a_rather_long_directory_name_to_pass_max_path");
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("from.txt"), "deep").unwrap();

        copy(&dir.join("from.txt"), &dir.join("to.txt"), &Progress::default(), &AtomicBool::new(false)).unwrap();

        assert_eq!(std::fs::read_to_string(dir.join("to.txt")).unwrap(), "deep");
    }
}
