//! `litastum-window`: opens litastum in a window of its own -- a new
//! Windows Terminal window with one tab, or a plain console window where
//! Windows Terminal isn't installed. A Windows GUI-subsystem program, so
//! double-clicking it doesn't flash a console of its own first. litastum
//! itself stays a console app. History: docs/history/launching.md.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::Command;


fn main() {
    let Some(litastum) = litastum_exe() else {
        return;
    };
    let start_dir = std::env::current_dir().unwrap_or_else(|_| litastum.parent().map(Path::to_path_buf).unwrap_or_default());
    launch(&litastum, &start_dir);
}


/// `litastum.exe` next to this launcher -- the two ship together.
fn litastum_exe() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(dir.join(if cfg!(windows) { "litastum.exe" } else { "litastum" }))
}


#[cfg(windows)]
fn launch(litastum: &Path, start_dir: &Path) {
    use std::os::windows::process::CommandExt;

    /// `CreateProcess`'s `CREATE_NEW_CONSOLE`: a console window of the
    /// child's own, since this launcher has none to share.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    if Command::new("wt.exe").args(windows_terminal_args(litastum, start_dir)).spawn().is_ok() {
        return;
    }
    let _ = Command::new(litastum).current_dir(start_dir).creation_flags(CREATE_NEW_CONSOLE).spawn();
}


/// Elsewhere there's no default terminal to pick: run it in this one.
#[cfg(not(windows))]
fn launch(litastum: &Path, start_dir: &Path) {
    let _ = Command::new(litastum).current_dir(start_dir).status();
}


/// `wt -w new new-tab --title litastum -d <dir> <litastum>`: always a new
/// window, never a tab in the user's open one. litastum sets its own title
/// too (`terminal_setup`); `--title` covers the moment before it starts.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_terminal_args(litastum: &Path, start_dir: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = ["-w", "new", "new-tab", "--title", "litastum", "-d"].iter().map(Into::into).collect();
    args.push(start_dir.into());
    args.push(litastum.into());
    args
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_new_window_in_the_start_directory() {
        let args = windows_terminal_args(Path::new(r"C:\tools\litastum.exe"), Path::new(r"W:\work"));
        let args: Vec<&str> = args.iter().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(args, ["-w", "new", "new-tab", "--title", "litastum", "-d", r"W:\work", r"C:\tools\litastum.exe"]);
    }
}
