# History (`command_line.rs`) — F9 → Commands → History, landed, gaps left

Every command actually run from the command line is recorded
(`command_line::record_history`, `App::command_history`, capped at
`theming::config::limits().max_command_history` (50 by default, see
[[litastum-config]]), skips an immediate repeat) — F9 → Commands →
History lists them,
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
- [x] **Results now scroll** -- reported directly, with a screenshot:
      the popup grew to fit however many entries matched instead of
      staying a fixed size, and once that ran past the terminal's own
      height the actually-selected entry (far down a long/unfiltered
      history) was clipped off past the popup's own bottom border with
      no way to see it. `ui/command_line.rs::draw_command_history` now
      renders a fixed-height (`HISTORY_HEIGHT`, matching
      `ui/find_file.rs::RESULTS_HEIGHT`'s own value) popup with a real
      `ListState` tracking the selected index, the same "`List` with no
      `ListState` doesn't auto-scroll" fix already landed for
      `ui/find_file.rs`'s own results list and `ui/theme_menu.rs`'s
      picker. Also widened from 60 to 70 columns in the same pass -- a
      real typed command (a long `svn`/`git` invocation, a deep path)
      routinely ran past 60 and got clipped mid-line.
- [x] **`Rounded` popup style actually applies now** -- follow-up
      report right after the scroll fix above: this was the one popup
      `ui::draw`'s own match arm never passed `app.popup_style` into,
      so it stayed hard-coded to `Classic`-only chrome regardless of
      the F9 -> Options -> UI setting. Migrated onto `popup::draw_frame`
      the same way `TODO/code-quality.md`'s own `ui/popup.rs` migration
      pass did for every other popup -- see that entry for why this one
      got missed the first time.
