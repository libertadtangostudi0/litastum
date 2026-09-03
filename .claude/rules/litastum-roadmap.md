# litastum: staged roadmap

Build in this order; each stage is independently usable.

0. **Skeleton** — done: `crossterm` + `ratatui` + `directories` +
   `color-eyre`.
1. **Dual-pane file browser (MVP)** — done: `std::fs` listing, sort
   (dirs first, case-insensitive name), arrow-key navigation, `Tab` to
   switch panels, `Enter` to descend/ascend.
2. **F4 editor, minimal version** — done: shell out to `$EDITOR` (or
   `notepad`/`nano` fallback), suspending/restoring the TUI around the
   external process.
3. **Built-in editor** — multi-column panel layout done (see
   [[litastum-ui-theme]], tracked in `ARCHITECTURE.md`). Editor itself:
   F4 now opens `editor.rs` (backed by `tui-textarea` — see
   [[litastum-stack]] for why, and for the `ratatui`/`crossterm` version
   pin this required) instead of shelling out. Still missing: syntax
   highlighting (`syntect`), a confirm-before-discard prompt on `Esc`
   with unsaved changes, and handling for files that aren't valid UTF-8.
4. **Scripting / user menu / macros** — see [[litastum-stack]] for the
   `rhai` vs `mlua` decision. Scripts should emit typed
   `Message`/`Command` values that the core executes — no direct
   filesystem/process access from scripts.
5. **Live config reload** — `notify` crate (cross-platform watcher).
6. **VFS** — archives and SFTP, see [[litastum-stack]] for the specific
   crates and why.
7. **Nice-to-haves** — image previews, WASM plugins. See
   [[litastum-stack]] for crate choices.
