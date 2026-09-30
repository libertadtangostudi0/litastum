use std::path::PathBuf;

/// Where the `*.txt` history files (command line, editor search, Find file
/// fields) live: `history/` in the project root, via `CARGO_MANIFEST_DIR`
/// baked in at compile time -- independent of the launch directory and
/// never outside the project tree. In tests, always a fresh temp directory,
/// so a test never touches the real `history/` (like `config_dir`).
/// History: docs/history/history-files.md.
pub(crate) fn history_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("history"))
    }
}
