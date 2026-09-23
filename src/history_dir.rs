use std::path::PathBuf;

/// Directory the persisted `*.txt` history files (command line, editor
/// search, Find file name/content) live under — `history/` directly
/// under this project's own repo root (`W:\rust\litastum\history\` for
/// this checkout), never the OS config dir and never the process's own
/// launch-time current directory either.
///
/// **History**: this used to be a handful of plain files resolved
/// relative to the process's own cwd, unconditionally — for at least
/// one real user that cwd turned out to sit inside a `%TEMP%`-adjacent
/// directory, and cleaning it out (an entirely reasonable thing to do
/// to a temp folder) silently deleted the whole command history right
/// along with it. A first fix moved these into the OS config dir
/// (`theming::config::config_dir`, the same place `config.json`/themes
/// already live) — reverted per explicit direction: this project's own
/// standing rule is to never have litastum read or write anything
/// outside its own directory tree, and the OS config dir is squarely
/// outside it. A second fix anchored to the running executable's own
/// directory instead (`target/debug/history/`, wherever `cargo build`
/// happened to put the binary) — also not what was actually asked for,
/// per explicit correction: the wanted location is a `history/` folder
/// directly in the project root, not wherever a given build's output
/// happens to land.
///
/// `CARGO_MANIFEST_DIR` (`env!`, resolved once at *compile* time to
/// this crate's own root -- `Cargo.toml`'s own directory) is what
/// actually gives that: a fixed, absolute path baked into the binary
/// itself, so it's found identically regardless of which directory the
/// process was launched from or which directory a panel is currently
/// browsing -- the property "next to the executable" was also reaching
/// for, just anchored to the right place this time.
///
/// **Always a fresh, isolated temp directory in a test build**,
/// unconditionally — same reasoning `theming::config::config_dir`'s own
/// doc comment already explains for its own `None`-in-test rule: a
/// test's result (and this repo's own working tree) must never depend
/// on, or write into, this project's real `history/` folder.
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
