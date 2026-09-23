use std::path::PathBuf;

/// Resolves `name` (a bare executable name typed with no path, e.g.
/// `"devenv"` or `"devenv.exe"`) through the Windows "App Paths"
/// registry mechanism --
/// `HKEY_CURRENT_USER`/`HKEY_LOCAL_MACHINE`
/// `SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\<name>.exe`'s
/// default value.
///
/// Reported directly against `devenv.exe`: Visual Studio's installer
/// registers itself here instead of adding its own install directory
/// to `PATH` -- the Explorer "Run" box and `ShellExecute` both consult
/// this key, but `cmd.exe`'s own bare-command search (what
/// `append_command_line` hands a typed line to) never does, so a name
/// that "just works" everywhere else came back
/// `'devenv.exe' is not recognized...` through litastum. `HKEY_CURRENT_USER`
/// is checked first, matching this key's own documented precedence
/// (a per-user registration should win over a machine-wide one).
///
/// Only ever returns a path that actually exists on disk right now --
/// a stale registry entry left behind by an uninstalled program
/// shouldn't make litastum hand `cmd.exe` a path that will just fail
/// to launch anyway, when leaving the original bare name in place would
/// at least get `cmd.exe`'s own real "not recognized" error instead of
/// a confusing one pointing at a path that no longer exists.
#[cfg(windows)]
pub(super) fn resolve(name: &str) -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let value_name = if name.to_lowercase().ends_with(".exe") { name.to_string() } else { format!("{name}.exe") };
    let subkey = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{value_name}");

    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().find_map(|hive| {
        let raw: String = RegKey::predef(hive).open_subkey(&subkey).ok()?.get_value("").ok()?;
        let path = PathBuf::from(unquote(&raw));
        path.is_file().then_some(path)
    })
}

/// Strips a surrounding pair of `"` characters, if present, plus any
/// incidental whitespace outside them.
///
/// Confirmed directly against a real `devenv.exe` entry: `App Paths`'
/// default value is commonly stored as a full quoted command-line-style
/// string (`"C:\...\devenv.exe"`, the quote characters actually part of
/// the registry value, not just how some tool happens to print it) --
/// the same convention lets a consumer splice the value straight into a
/// command line with spaces already protected. Left un-stripped,
/// `PathBuf` treats the quotes as literal filename characters and
/// `resolve`'s own `is_file()` check always fails, silently discarding
/// a real, valid registration -- this was the actual reason resolving
/// `devenv` came back empty even though the key genuinely exists.
#[cfg(windows)]
fn unquote(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(trimmed)
}

/// App Paths is a Windows-only registry mechanism -- Unix has no
/// equivalent (`PATH` is the only lookup that ever applies there), so
/// this always reports "nothing registered under that name" rather
/// than every call site needing its own separate `#[cfg(windows)]`.
#[cfg(not(windows))]
pub(super) fn resolve(_name: &str) -> Option<PathBuf> {
    None
}


#[cfg(all(test, windows))]
mod tests {
    use super::*;

    mod unquote_tests {
        use super::*;

        #[test]
        fn strips_a_surrounding_quote_pair() {
            assert_eq!(unquote(r#""C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\devenv.exe""#), r"C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\devenv.exe");
        }

        #[test]
        fn leaves_an_already_unquoted_value_untouched() {
            assert_eq!(unquote(r"C:\Windows\notepad.exe"), r"C:\Windows\notepad.exe");
        }

        #[test]
        fn trims_incidental_whitespace_outside_the_quotes() {
            assert_eq!(unquote("  \"C:\\tools\\thing.exe\"  "), r"C:\tools\thing.exe");
        }
    }
}
