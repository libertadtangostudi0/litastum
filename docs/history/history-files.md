# App data location (`app_data.rs`) -- history

- **Relative to the working directory at first.** For one user that was
  inside a temp-like directory, and cleaning it deleted the whole
  command history.
- **The OS config directory** (next to `config.json`) was the first fix
  -- reverted: litastum must not read or write outside its own tree.
- **Next to the executable** (`target/debug/history/`) was the second --
  also not what was asked: the folder belongs in the project root.
- **`CARGO_MANIFEST_DIR`**, baked in at compile time, gives exactly that,
  whatever the launch directory.

## `appdata/`: all app data in the project until there's an installer

`config_dir()` still defaulted to `%APPDATA%\litastum\` (via
`ProjectDirs`) whenever `LITASTUM_CONFIG_DIR` wasn't set -- outside the
project, which the app must not touch before it has an installer. Now
`config.json`, user themes, the common F2 menu and `history/` all live
in `appdata/` at the project root (`src/app_data.rs`, replacing
`history_dir.rs`), laid out the way `%APPDATA%\litastum\` will be, so
the installer can move it over as-is. `LITASTUM_CONFIG_DIR` went away
with it: the path is always inside the project now. The existing
`config.json`, `history/` and root `LitastumMenu.toml` were moved in.
