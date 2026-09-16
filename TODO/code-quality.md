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
- [x] Clamped-list-cursor logic (`selected = selected.saturating_sub(1)`
      / `if selected + 1 < len { selected += 1 }`) was duplicated
      across eight sites by the time this was picked up (the original
      audit found five; three more had been added since -- `editor/
      keymap_menu.rs::EditorKeymapMenu`, `editor/menu.rs::EditorMenu`),
      already in two different shapes: a method on the menu's own
      struct (`theme_menu.rs`, `menu.rs::MainMenu`, `popup_style_menu.rs`,
      the two `editor/` ones above) vs. inline in the key handler
      (`shell.rs`, `command_line/history.rs`, `drive_menu.rs`). Replaced
      with two free functions, `list_cursor::{move_up, move_down}` (new
      top-level module, same "small shared utility, no crate it belongs
      under" placement as `text_field.rs`) -- a plain function pair
      rather than a `ClampedCursor` wrapper type, since every call site
      already owns a bare `selected: usize` field directly and
      restructuring eight structs' own fields to hold a wrapper instead
      would have been a much bigger, less obviously-worth-it change for
      the same result. Every one of the eight sites now either
      delegates its own `move_up`/`move_down` method to these, or calls
      them directly inline where that was already the shape; behavior
      unchanged, confirmed by the full existing test suite still
      passing with no edits needed to any of it.
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
- [x] `menu.rs`, `shell.rs`, and `drive_menu.rs` were structurally
      near-identical simple list popups (Up/Down/Enter/Esc, one
      `List` + a footer hint), each redefining the same
      `(count + 4).clamp(6, area.height)`-shaped height formula and
      layout -- `popup_style_menu.rs` (added for F9 → Options → UI) was
      a fourth copy, and two more had joined the pile since (`editor/
      menu.rs`'s own F9, `editor/keymap_menu.rs`'s `Standard`/`Vim`
      picker), six sites total by the time this was picked up. Replaced
      with `popup::draw_list_popup` (title, width, pre-formatted
      `labels: &[String]`, selected index, and the two hint
      descriptions -- see its own doc comment for why it takes final
      label strings rather than raw domain data: what each caller needs
      to turn an item into a label varies too much, from `drive_menu.rs`'s
      own multi-column `format!` to `popup_style_menu.rs`'s "(current)"
      suffix, for a shared formatter to be worth it). Every one of the
      six `draw_*` functions shrank to essentially "build the labels,
      call `draw_list_popup`"; behavior unchanged for five of them
      (confirmed by their own existing tests, none needing edits), the
      sixth (`menu.rs`) had none to begin with. Also gained its own
      direct test coverage (`ui/popup.rs`'s own test module) that didn't
      exist for the duplicated logic before, including an empty-list
      case none of the six original call sites had ever been tested
      against.

**Checked clean, no action needed**: `.unwrap()`/`.expect()`/`panic!`
usage outside tests is essentially nonexistent (only on hardcoded
constants where a panic can't actually occur); no dead code found --
every public helper surviving the earlier file-size decomposition pass
is actually called from somewhere.
