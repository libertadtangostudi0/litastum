# TODO

Near-term, actionable items. Full staged plan:
`.claude/rules/litastum-roadmap.md`. Design for the items below:
`ARCHITECTURE.md`.

## Stage 3 UI pass (in order — see ARCHITECTURE.md "suggested implementation order")

- [x] `theme.rs` — color constants from `.claude/rules/litastum-ui-theme.md`
- [x] Column-major `Panel` navigation (`columns` field, row/col index math)
- [x] `ui.rs`: replace `List` with a manual column-major grid renderer
- [x] `keymap.rs` + `command.rs` — extract key resolution/execution out of `main.rs::handle_event`
- [x] Wire Left/Right column movement through the new command layer

## Built-in editor (F4, `editor.rs`, `tui-textarea`) — MVP landed, gaps left

- [ ] Confirm-before-discard prompt when closing (`Esc`) with unsaved
      changes — currently discards silently, no modal system exists yet
      to ask
- [ ] Handle non-UTF-8 / binary files without just silently doing
      nothing on F4 — at least a status-bar message once one exists
- [ ] Syntax highlighting (`syntect`) — deferred, not started
- [ ] `ratatui`/`crossterm` are pinned to 0.29/0.28 for `tui-textarea`
      compat — see [[litastum-stack]] — bump back to 0.30/0.29 once
      `tui-textarea` supports it

## RESOLVED: Ctrl+S / Ctrl+C / Ctrl+V / Ctrl+X (2026-09-04)

Confirmed working after splitting editor key resolution into
`editor_keymap.rs` (mirroring `keymap.rs`/`command.rs`) and adding
`logging.rs`. Root cause was never pinned down with certainty (the fix
landed together with the uppercase-letter match and the mode-borrow
restructuring in `main.rs::handle_editor_key`) — if a similar "key does
nothing" report comes up again, `logs/litastum.log` now has `debug!` on
every key event and resolved command in both modes, and `warn!`
specifically when `arboard::Clipboard::new()` fails, to make it
diagnosable without guessing.

The logging infrastructure stays (`logging.rs`, size-capped at 30 MB —
see `SizeCappedFile`) for the next time something like this happens.

## Next up

- [ ] Show item count / free space in each panel's footer (mockup has
      this; not yet in `Panel`/`ui.rs`)
- [ ] Use `Entry::size`/`Entry::modified` for something, or drop them —
      currently dead code (`cargo build` warns on this)

## Housekeeping

- [ ] `Cargo.toml`: replace `YOUR_USERNAME` placeholder in `repository`
