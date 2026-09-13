# F9 menu — Main → Commands/Options landed, real menu still to do

- [x] `menu.rs` — F9 opens a top menu (`Main`: `Commands`, `Options`)
      that descends into a submenu instead of jumping straight to a
      leaf action. `Esc` backs up one level at a time rather than
      always closing outright. Dispatch on `Select` is matched on
      `(level, item label)` rather than a positional index, so
      reordering `MenuLevel::items` can't silently wire the wrong
      action to a key.
- [x] `Options` → `Color schemes` (`Mode::ThemeMenu`, unchanged) and
      `Save setup` — Far Manager's own Shift+F9, reached only through
      the menu here (no global hotkey binding, wasn't asked for):
      persists the current session's choices to `config.json` on
      demand rather than every choice auto-persisting immediately the
      way the theme picker's own does. So far, that's just the active
      shell profile (`config.rs::save_setup`/`load_active_shell`,
      applied back at startup in `main.rs` if the saved name still
      matches a built-in profile) — the one setting that wasn't already
      being persisted somewhere.
- [x] `Commands` → `Find file` and `History` — see
      [find-file.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/find-file.md) and [history.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/history.md).

`menu.rs` is deliberately *just* enough structure to reach what's
actually been asked for so far — not a real F9 top-menu bar (Far
Manager's own F9 is Left/Files/Commands/Options/View/Right, each with
real submenus of their own, and `Commands`/`Options` here only have two
items apiece so far). Gaps if this grows toward that:

- [ ] `MainMenu::back()` hardcodes every non-`Main` level's parent as
      `Main` — fine while the menu stays two levels deep, would need
      each level to know its own parent if a third level is ever added
- [ ] No keyboard shortcut letters (Far-style `S` for Settings, etc.) —
      `Up`/`Down`/`Enter` only
- [ ] Ctrl+V-over-selection replacing text, syntax highlighting for a
      *currently open* editor when its theme changes live — out of
      scope for the picker itself, listed here only because "apply
      live" doesn't retroactively re-theme an already-open `Editor`
      (the new `editor_theme` applies to the next file opened)
- [ ] No preview while browsing the theme list — colors only change
      once applied (Enter/I/E), not as you move the cursor over each
      name
- [ ] Long theme lists aren't scrolled, just clamped to the terminal
      height — fine for a handful of files, not for many
