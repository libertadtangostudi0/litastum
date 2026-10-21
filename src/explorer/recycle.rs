//! `F8` sends files to the Recycle Bin (requested: a delete could be
//! undone); `Shift+Del` still deletes them for good, as in Far. The
//! sending itself is `file_job::recycle_bin`'s. Outside Windows, and in
//! tests (never the real Recycle Bin of whoever runs them), a delete is
//! for good.

/// Whether `F8` can send to a Recycle Bin here -- the prompt says which
/// it is.
pub fn available() -> bool {
    cfg!(all(windows, not(test)))
}
