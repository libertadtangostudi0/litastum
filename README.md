# litastum

A cross-platform, keyboard-driven dual-pane file manager written in
Rust — inspired by Far Manager, not a clone of it.

Runs as a real terminal application (works over SSH, in any terminal
emulator) via `crossterm` + `ratatui`, rather than as a windowed GUI
app.

## Status

Early scaffold. Currently working:

- Dual-pane directory browsing (`std::fs`, sorted: directories first,
  then case-insensitive by name)
- Arrow-key navigation, `Tab` to switch the active panel, `Enter` to
  descend into a directory or go up via `..`
- `F4` opens the file under the cursor in `$EDITOR` (falls back to
  `notepad` on Windows, `nano` elsewhere)
- `F10` / `q` to quit

See `CLAUDE.md` for the full staged roadmap and the design decisions
behind the stack choices.

## Build & run

```sh
cargo run
```

Requires a reasonably current stable Rust toolchain (install via
[rustup](https://rustup.rs) if you don't have one).

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
