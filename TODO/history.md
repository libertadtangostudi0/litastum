# History (`command_line.rs`) — F9 → Commands → History, landed, gaps left

Every command actually run from the command line is recorded
(`command_line::record_history`, `App::command_history`, capped at 50,
skips an immediate repeat) — F9 → Commands → History lists them,
`Enter` copies the highlighted one into the command line (doesn't
auto-run it — recalling something to tweak before running felt like
the more common case, and never auto-executing is the safer default
regardless), `Esc` cancels. Far Manager's own Alt+F8 — reached only
through the menu here, no global hotkey, and deliberately *not*
`Up`-arrow recall: arrows stay bound to panel navigation on the
always-live command line (see [[litastum-command-line]]), which is
exactly why a menu-driven popup was the way to add history at all
without reopening that conflict.

- [ ] Session-only — not persisted to `config.json`, resets to empty
      every run
- [ ] No search/filter within a long history list — just `Up`/`Down`
- [ ] `cd`s are recorded like any other command, `cls`/`clear` are too
      — no filtering of "boring" entries
- [ ] Results aren't scrolled, just clamped to the terminal height
