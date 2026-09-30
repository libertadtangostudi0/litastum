use std::path::Path;
use std::process::{Child, Command};

/// The command (not run) that opens `path` like a double-click in the
/// file manager: `explorer.exe` on Windows, `open` on macOS, `xdg-open`
/// elsewhere (absent on headless systems -- accepted). Split out so tests
/// can inspect it.
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
/// the command line's own external process spawns:
/// a failure here (the OS command itself missing, or whatever it
/// launches failing) isn't something this app can usefully react to
/// beyond logging it, so it's never propagated up as an
/// application-level error.
pub fn open(path: &Path) -> std::io::Result<Child> {
    build_open_command(path).spawn()
}

/// The command (not run) that opens a URL. On Windows `cmd /C start "" <url>`
/// rather than `explorer.exe <url>`, which goes through the running
/// Explorer over IPC and felt slow for a `Ctrl`+click; `start` calls
/// `ShellExecute` directly. `""` is `start`'s title argument, needed or a
/// quoted target is taken as the title. `open`/`xdg-open` elsewhere.
/// History: docs/history/markdown-preview.md.
fn build_open_url_command(url: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
}

/// `Ctrl`+click / `l`-search `Enter` on a Markdown link that resolved to
/// a real URL (`markdown_preview::links::LinkTarget::Url`) -- see
/// `build_open_url_command`'s own doc comment for why this is a
/// distinct code path from `open` above rather than reusing it.
pub fn open_url(url: &str) -> std::io::Result<Child> {
    build_open_url_command(url).spawn()
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

    #[test]
    #[cfg(windows)]
    fn windows_opens_a_url_via_cmd_start_not_explorer() {
        let command = build_open_url_command("https://example.com");
        assert_eq!(command.get_program(), "cmd");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec!["/C", "start", "", "https://example.com"]);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_opens_a_url_with_open() {
        let command = build_open_url_command("https://example.com");
        assert_eq!(command.get_program(), "open");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec!["https://example.com"]);
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn other_unix_opens_a_url_with_xdg_open() {
        let command = build_open_url_command("https://example.com");
        assert_eq!(command.get_program(), "xdg-open");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec!["https://example.com"]);
    }
}
