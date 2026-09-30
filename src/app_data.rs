use std::path::PathBuf;

/// The app's own data directory: `appdata/` in the project root, anchored
/// with `CARGO_MANIFEST_DIR` so it doesn't depend on the launch directory.
/// Holds what an installed build will keep in `%APPDATA%\litastum\`
/// (`config.json`, user `themes/`, the common F2 menu, `history/`), laid
/// out the same way so it can be moved there as-is once an installer
/// exists. Until then nothing is read or written outside the project.
///
/// `None` in tests, so a test never touches the real files.
/// History: docs/history/history-files.md.
pub(crate) fn app_data_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("appdata"))
    }
}


/// Where the `*.txt` histories live (command line, editor search, Find
/// file fields): `appdata/history/`.
pub(crate) fn history_dir() -> Option<PathBuf> {
    app_data_dir().map(|dir| dir.join("history"))
}
