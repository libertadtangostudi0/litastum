use std::path::PathBuf;

/// Resolves a bare executable name (`devenv`, `devenv.exe`) through the
/// Windows "App Paths" registry key (`HKCU`, then `HKLM`,
/// `SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\<name>.exe`).
/// Explorer's Run box and `ShellExecute` consult it, `cmd.exe` doesn't --
/// Visual Studio registers only there. Returns a path only if it exists
/// now, so a stale entry still gets `cmd.exe`'s own "not recognized".
/// History: docs/history/command-execution.md.
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

/// Strips surrounding `"` and outer whitespace. App Paths values are
/// often stored quoted (`"C:\...\devenv.exe"`); unstripped, the quotes
/// became part of the file name and the `is_file()` check discarded a
/// valid registration.
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
