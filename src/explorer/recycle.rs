//! `F8` sends files to the Recycle Bin (requested: a delete could be
//! undone); `Shift+Del` still deletes them for good, as in Far. Through
//! the shell's own file operation, as Explorer does. Outside Windows, and
//! in tests (never the real Recycle Bin of whoever runs them), a delete
//! is for good.

use std::io;
use std::path::Path;


/// Whether `F8` can send to a Recycle Bin here -- the prompt says which
/// it is.
pub fn available() -> bool {
    cfg!(all(windows, not(test)))
}


/// Sends `path` -- a file or a directory, with all in it -- to the
/// Recycle Bin.
#[cfg(all(windows, not(test)))]
pub fn to_recycle_bin(path: &Path, _is_dir: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};

    let absolute = std::path::absolute(path)?;
    // A list of paths, each ending in a NUL, the list in another.
    let mut from: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `operation` and the path list it points at live through
    // the call.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(io::Error::other(format!("couldn't move it to the Recycle Bin (shell error {result:#x})")));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err(io::Error::other("moving it to the Recycle Bin was cancelled"));
    }
    Ok(())
}


/// No Recycle Bin: deleted for good (`available` says so).
#[cfg(not(all(windows, not(test))))]
pub fn to_recycle_bin(path: &Path, is_dir: bool) -> io::Result<()> {
    delete_for_good(path, is_dir)
}


/// Deletes `path` for good -- a directory (as the panel lists it) with
/// all in it.
pub fn delete_for_good(path: &Path, is_dir: bool) -> io::Result<()> {
    if is_dir {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
