# Next up

- [x] **Startup showed one narrow single-file-per-row column, stuck
      until the first keypress** — `Panel::new()` defaults `columns: 1`/
      `visible_rows: 0` (`column_height()`'s own "not yet known"
      sentinel) until `main.rs::run()`'s loop feeds back the real,
      `ui::draw`-computed values -- but that feedback only ever reaches
      the draw call *after* the one that already rendered with the
      stale defaults, and `wait_for_event` blocks for real input in
      between, so the wrong layout wasn't just a one-frame flash, it
      stuck around until the user pressed something. Fixed with one
      extra seed draw+apply cycle in `run()`, before the interactive
      loop's own first frame, so real values are already in place by
      the time anything is actually shown.
- [ ] Scrolling/pagination for the file panel's entry list — requested
      directly. Currently the 2-column column-major grid
      (`.claude/rules/litastum-ui-theme.md`) has no scroll offset at
      all, so a directory with more entries than fit the panel's own
      height is presumably just clamped/cut off rather than scrolling
      to follow the cursor — needs checking exactly what `ui.rs`'s
      current rendering does today before designing the fix. Likely
      needs a per-`Panel` scroll-offset field, kept in sync with cursor
      movement (`PageUp`/`PageDown` already exist as bindings elsewhere
      in this codebase — worth checking whether the panel honors them
      at all right now), and has to interact correctly with the
      column-major (fill column 1 top-to-bottom, then column 2) layout,
      not just a naive single-column scroll.
- [x] Multi-select in the file panel — landed as `Shift+A` (select all)
      and `Shift+Up`/`Down`/`Left`/`Right` (toggle-and-move, whole-
      column for Left/Right) rather than the `Ins`+`Shift+arrow`
      combination originally sketched here — see
      [copy-move.md](copy-move.md) for the full binding story and
      `panel/marks.rs`. Marked entries
      render in `theme.warning` (the "attention/marked" color the
      original UI-theme plan had already named but never wired up),
      overriding type-based coloring, rather than a distinct new
      `Theme` field.
- [ ] Show item count / free space in each panel's footer (mockup has
      this; not yet in `Panel`/`ui.rs`)
- [ ] Use `Entry::size`/`Entry::modified` for something, or drop them —
      `#[allow(dead_code)]` on `Entry` silences the warning deliberately
      in the meantime, not a fix in itself
- [x] `Alt` swapping the F-key hint bar to a second row of labels — the
      earlier bullet here guessed this meant a console/"additional
      screen" toggle; a follow-up screenshot clarified it's Far
      Manager's actual bottom bar changing labels while `Alt` is held.
      Landed as `App::alt_held` (set from every key event's modifiers
      in `main.rs::handle_event`) driving `ui::draw_function_keys`'s
      choice between `DEFAULT_LABELS`/`ALT_LABELS`, plus the one label
      that's an actual rebinding rather than cosmetic: `Alt+F7` opens
      Find file directly (`command_line.rs`, same special-casing
      pattern as `Shift+F6`), matching real Far's own global shortcut
      for it — previously only reachable through F9 → Commands → Find
      file. **Known limitation, not a bug**: terminals (including the
      Windows Console API this project mainly targets) generally don't
      deliver a standalone press/release event for a bare modifier key
      on its own — only modifier flags riding along with an actual
      keypress. So the alt row appears the instant `Alt+`-something is
      pressed, but only reverts on the *next* key event without `Alt`,
      not the instant `Alt` alone is released (`App::alt_held`'s own
      doc comment has the full explanation). No test coverage added —
      both the modifier-tracking assignment and the `Alt+F7` branch
      live in code that already has no unit tests for the same reason
      (`main.rs::handle_event`/`command_line.rs::handle_browsing_key`
      need a real `Terminal`)
- [x] `Alt+F1`/`Alt+F2` — Far Manager's own per-panel drive-switching
      menu (`explorer/drive_menu.rs`, `ui/drive_menu.rs`): `Alt+F1`
      always targets the left panel, `Alt+F2` always the right one
      (`DriveMenu::target_panel`, fixed at open time — independent of
      which panel currently has focus, matching real Far), lists
      logical drives with type and total/free space
      (`GetLogicalDrives`/`GetDriveTypeW`/`GetDiskFreeSpaceExW` via
      `windows-sys`, a new Windows-only dependency), `Enter` navigates
      via the existing `Panel::change_dir`. Windows-only real
      enumeration; the Unix build gets a single `"/"` entry rather than
      real mount-point enumeration — same scope cut as `shell.rs`'s
      Unix shell-profile fallback, not attempted here either
- [ ] **Diff view mode + conflict resolver** — placeholder, rules/scope
      to be filled in later (not yet specified: whether this is a
      standalone F-key-triggered mode, an editor overlay, how it hooks
      into VCS state if at all, two-way vs. three-way, resolution UI).
      Don't start implementation from this bullet alone. See
      [git-integration.md](git-integration.md) — likely related (a
      conflict resolver needs to know a file's conflicted-hunk
      structure from somewhere, which is git-specific).
