# TODO

Near-term, actionable items. Full staged plan:
`.claude/rules/litastum-roadmap.md`. Design for the items below:
`ARCHITECTURE.md`.

## Stage 3 UI pass (in order — see ARCHITECTURE.md "suggested implementation order")

- [ ] `theme.rs` — color constants from `.claude/rules/litastum-ui-theme.md`
- [ ] Column-major `Panel` navigation (`columns` field, row/col index math)
- [ ] `ui.rs`: replace `List` with a manual column-major grid renderer
- [ ] `keymap.rs` + `command.rs` — extract key resolution/execution out of `main.rs::handle_event`
- [ ] Wire Left/Right column movement through the new command layer

## Housekeeping

- [ ] `Cargo.toml`: replace `YOUR_USERNAME` placeholder in `repository`
