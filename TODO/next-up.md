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
- [x] **Holding a scroll-wheel/touchpad gesture, or an arrow/Page key,
      then quickly reversing direction, felt sluggish to respond** --
      reported twice, first against touchpad scrolling, then more
      precisely against the built-in editor's own text caret ("каретка
      ввода текста... идёт за пределы страницы и продолжает ещё ехать и
      ехать... остановка при реверсе слишком медленная"). Root cause in
      both cases: `main.rs::run()`'s loop does a full `terminal.draw()`
      after *every single event* it handles, with no batching -- a
      touchpad's own scroll momentum, or the OS's key-repeat while an
      arrow/Page key is held, both generate a rapid burst of distinct
      events rather than one, so reversing direction mid-burst meant
      draining however many old-direction events were still queued
      (each getting its own full, sometimes expensive redraw) before the
      new direction was even read. Fixed the same way for both:
      `main.rs::drain_pending_mouse_events`/`drain_pending_navigation_keys`
      drain a same-direction burst without redrawing per event (so a
      long, uninterrupted scroll/hold still redraws promptly rather than
      once per tick), but stop *immediately* the moment a genuinely
      reversed direction is read, rather than waiting out the rest of
      the burst -- a real reversal is the one case where continuing to
      coalesce is actively wrong. The navigation-key drain only ever
      triggers for `Up`/`Down`/`PageUp`/`PageDown` (the keys actually
      meant to be held to move through a long list/document) --
      everything else, typing included, dispatches exactly as before.
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
      [copy-move.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/copy-move.md) for the full binding story and
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
- [ ] **Diff view mode + conflict resolver** — real spec now written up
      in [file-compare.md](file-compare.md): a two-panel, two-file,
      GitHub-diff-colored comparer (litastum's own editor themes, not a
      fixed palette) modeled loosely on real Far Manager's `merge.exe`,
      phase 1; a 3-way conflict resolver, phase 2, built on top once
      phase 1 is proven, not designed in detail yet. Don't start
      implementation until that document's own "Open questions" section
      (rendering approach, keybinding, how the second file is chosen,
      theming) is actually answered. See
      [git-integration.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/git-integration.md)
      too — likely related for phase 2 specifically (a conflict
      resolver needs to know a file's conflicted-hunk structure from
      somewhere, which is git-specific; phase 1 itself is VCS-agnostic).
