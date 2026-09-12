use std::path::Path;
use std::process::{Child, Command};

/// Builds (but doesn't run) the OS command that opens `path` the way a
/// real double-click in the platform's own file manager would --
/// launches whatever program is associated with a file, or opens a
/// window navigated to a directory. Split out from `open` below so
/// tests can inspect the constructed `Command` (program name,
/// arguments) without ever launching a real process.
///
/// - Windows: `explorer.exe <path>` -- handles a plain file path the
///   same way a double-click does (hands it to whatever's associated
///   with it), no separate `ShellExecute` call needed, and opens a
///   window at `path` when it's a directory.
/// - macOS: `open <path>` -- same shape, the platform's own equivalent.
/// - Other Unix: `xdg-open <path>` -- the desktop-agnostic
///   freedesktop.org convention. Not installed on a headless system,
///   but this project already accepts that class of limitation
///   elsewhere (see `shell.rs`'s own Unix shell-profile fallback).
fn build_open_command(path: &Path) -> Command {
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    let mut command = Command::new(program);
    command.arg(path);
    command
}

/// `Shift+Enter`: opens the entry under the cursor in the OS's own file
/// manager, fire-and-forget -- same reasoning as
/// `command_line.rs::run_command_line`'s own external process spawns:
/// a failure here (the OS command itself missing, or whatever it
/// launches failing) isn't something this app can usefully react to
/// beyond logging it, so it's never propagated up as an
/// application-level error.
pub fn open(path: &Path) -> std::io::Result<Child> {
    build_open_command(path).spawn()
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    #[cfg(windows)]
    fn windows_uses_explorer_with_the_path_as_its_only_argument() {
        let path = PathBuf::from(r"C:\Users\test\file.txt");
        let command = build_open_command(&path);
        assert_eq!(command.get_program(), "explorer");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec![path.as_os_str()]);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_uses_open_with_the_path_as_its_only_argument() {
        let path = PathBuf::from("/tmp/file.txt");
        let command = build_open_command(&path);
        assert_eq!(command.get_program(), "open");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec![path.as_os_str()]);
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn other_unix_uses_xdg_open_with_the_path_as_its_only_argument() {
        let path = PathBuf::from("/tmp/file.txt");
        let command = build_open_command(&path);
        assert_eq!(command.get_program(), "xdg-open");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec![path.as_os_str()]);
    }
}
