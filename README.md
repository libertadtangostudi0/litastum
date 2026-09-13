# litastum

A cross-platform, keyboard-driven dual-pane file manager written in
Rust — a Far Manager analog: same core workflow (dual panes, F-key
commands, a built-in editor), not a clone of it.

Runs as a real terminal application (works over SSH, in any terminal
emulator) via `crossterm` + `ratatui`, rather than as a windowed GUI
app.

## Status

Dual-pane browsing, a built-in editor, and Far Manager's core keyboard
workflow are all working:

- Column-major dual-pane browsing with multi-select, Copy/Move/Rename/
  Delete (`F5`/`F6`/`Shift+F6`/`F8`), `Shift+Enter` to open an entry in
  the OS's own file manager
- `F4` built-in editor (`edtui`) with syntax highlighting, search, and
  a standard (non-modal) keymap
- Always-live Far-style command line with history and Tab completion
- `F9` menu: color schemes, popup style (Classic/Rounded), saved setup
- `Ctrl+P` shell picker, `Alt+F1`/`F2` drive switcher, `Alt+F7` find
  file, `Alt+F8` command history

See `ARCHITECTURE.md` for the module map and `.claude/rules/*.md` for
the design decisions behind each area; `CLAUDE.md` for the staged
roadmap (scripting, live config reload, and a VFS layer are next).

## Build & run

```sh
cargo run
```

Requires a reasonably current stable Rust toolchain (install via
[rustup](https://rustup.rs) if you don't have one).

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](https://github.com/libertadtangostudi0/litastum/blob/main/LICENSE-MIT))

at your option.
