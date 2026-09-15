# Code quality

Findings from a best-practices review of `src/` (duplicated logic,
naming, error-handling consistency, dead code) — not urgent bugs, just
backlog items to pick up opportunistically.

- [x] **Scattered hardcoded tuning `const`s centralized into `config.json`**
      -- found during a perf/weak-spot audit pass: five unrelated caps
      (`command_line/history.rs::MAX_HISTORY`, `find_file/search.rs::
      MAX_RESULTS`/`MAX_VISITED`, `ui/panel.rs::MIN_COLUMN_WIDTH`,
      `markdown_preview.rs::PAGE_SIZE`, `logging.rs::MAX_LOG_BYTES`),
      each only ever changeable by editing source and rebuilding. Moved
      into a new `Limits` struct (`theming/config/limits.rs`), same
      known-default-with-`config.json`-override shape every other
      setting in that file already has -- see `.claude/rules/
      litastum-config.md` for the full table and design. Also
      documented this project's environment-variable convention
      (`LITASTUM_` prefix, `RUST_LOG` the one deliberate exception)
      in the same new rules file, prompted by the same audit noticing
      there was nowhere written down.
- [x] `ui/popup.rs` migration -- at review time, only `confirm.rs`'s
      delete popup, `theme_menu.rs`, and `editor_find.rs` used the
      shared `draw_frame`/`key_pill` chrome; `menu.rs`, `shell.rs`,
      `drive_menu.rs`, `find_file.rs`, and `confirm.rs`'s transfer popup
      still hand-rolled their own `Block::borders(ALL)`. Landed as part
      of the F9 → Options → UI feature (`PopupStyle::{Classic, Rounded}`):
      `draw_frame` now takes a `style` and renders either look, and all
      of the above route through it -- see
      `.claude/rules/litastum-popup-design.md` and this file's own git
      history for the design. `editor_find.rs`'s minimal search box was
      deliberately kept outside this (see its own doc comment).
      **One popup was actually missed by this pass**, found later from
      a direct report ("ui для окна rounded не работает"):
      `ui/command_line.rs::draw_command_history` (Alt+F8) still
      hand-rolled its own `Classic`-only `Block::borders(ALL)`, so
      switching to `Rounded` visibly did nothing for it -- the one
      popup `ui::draw`'s own match arm *didn't* pass `app.popup_style`
      into. Migrated the same way the rest of this pass did, alongside
      a real scroll fix for the same popup (see `TODO/history.md`).
- [ ] Clamped-list-cursor logic (`selected = selected.saturating_sub(1)`
      / `if selected + 1 < len { selected += 1 }`) is duplicated ~5
      times, already in two different shapes: a method on the menu's
      own struct (`theme_menu.rs::move_up`/`move_down`,
      `menu.rs::MainMenu::move_up`/`move_down`,
      `popup_style_menu.rs::PopupStyleMenu::move_up`/`move_down`) vs.
      inline in the key handler (`shell.rs`, `command_line/history.rs`,
      `drive_menu.rs`). A shared helper (free function or a small
      `ClampedCursor` type) would remove the duplication and settle on
      one shape.
- [ ] Asymmetry between the two single-line text-entry fields:
      `confirm.rs`'s transfer-destination field has `Ctrl+Left`/`Right`
      (word-wise movement), `Shift`-selection, and `Delete`
      (`text_field.rs`); `find_file.rs`'s search box only has
      backspace/insert/plain arrows. Not documented anywhere as a
      deliberate scope cut -- either narrow it down as an accepted
      simplification (and say why, in `find-file.md` or here) or extend
      the search box to reuse `text_field.rs` like the destination field
      does.
- [ ] `command_line/history.rs::handle_history_key` uses a `matches!`
      guard plus three separate `unreachable!()` calls inside its
      `match`, instead of the single `let Mode::X(y) = &mut app.mode
      else { return Ok(()) }` guard idiom every sibling handler in this
      codebase uses (`theme_menu.rs`, `menu.rs`, `popup_style_menu.rs`,
      `shell.rs`, ...). Works today, but only because nothing between
      the `matches!` check and the `match` arms changes `app.mode` --
      fragile against a future edit; switching it to the shared idiom
      removes the possibility of that footgun entirely.
- [ ] The selected-row style (`fg(theme.text).bg(theme.current_row_bg)
      .add_modifier(Modifier::BOLD)`) and the footer-hint spans
      (`Span::styled("Enter", fg(theme.accent).BOLD)` +
      `Span::styled(" label  ", fg(theme.text_dim))`, repeated per hint)
      are hand-copied roughly 30 and 45 times respectively across
      `ui/*.rs`, even though `popup::key_pill` already exists for the
      hint case (just not adopted everywhere -- see the first item
      above, now resolved for the frame itself but not yet for the
      footer-hint styling). A small helper for each (a `selected_row_style`
      function, a `hint_span`/`hint_line` builder) would collapse most of
      this.
- [ ] The `let Mode::X(y) = &app.mode else { return Ok(()) }` (or
      `&mut`) guard appears roughly 59 times across the mode handlers.
      Not a problem on its own (it's the established, correct idiom --
      see the `history.rs` item above), but the sheer volume means any
      future change to that convention has to be hand-applied
      everywhere rather than in one place.
- [ ] `menu.rs`, `shell.rs`, and `drive_menu.rs` are structurally
      near-identical simple list popups (Up/Down/Enter/Esc, one
      `List` + a footer hint) that each redefine the same
      `(count + 4).clamp(6, area.height)`-shaped height formula and
      layout. `popup_style_menu.rs` (added for F9 → Options → UI) is a
      fourth copy of the same shape. A shared `draw_list_popup` (title,
      items, selected index, theme, style) could replace all four
      call sites.

**Checked clean, no action needed**: `.unwrap()`/`.expect()`/`panic!`
usage outside tests is essentially nonexistent (only on hardcoded
constants where a panic can't actually occur); no dead code found --
every public helper surviving the earlier file-size decomposition pass
is actually called from somewhere.
