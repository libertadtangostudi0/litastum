# Theming (`scheme.rs`, `config.rs`) — landed, gaps left

Windows Terminal-format color schemes drive the app's interface
(panels, borders, F-key bar, ...) and the editor's syntax highlighting
*independently* — `config.json`'s `interface_theme` and `editor_theme`
keys, matching how Far Manager itself keeps those two systems separate.
See [[litastum-theming]] for the full design and the mapping
conventions.

- [x] F9 → Options → Color schemes in-app theme picker (`theme_menu.rs`,
      reached through `menu.rs`) — lists `themes/*.json`, applies live
      (no restart) and persists to `config.json`. See
      [f9-menu.md](f9-menu.md) for how much of a real top-menu bar this
      actually is (not much)
- [ ] No hot-reload when a theme *file itself* is edited on disk while
      running (roadmap stage 5 territory, `notify` crate) — F9 re-scans
      the `themes/` directory each time it opens, but doesn't watch it
- [ ] Real terminal cursor *color* isn't driven by `cursorColor` (only
      shape is themed) — needs an OSC 12 escape sequence crossterm
      doesn't wrap
- [x] File-type coloring (`panel.rs::HighlightRole`) — archives,
      executables/scripts, and VCS metadata dirs (`.git`/`.svn`/`.hg`/
      `.bzr`) get distinct colors, Far Manager-style; reverses an
      earlier "no file-type color dots" decision (see
      [[litastum-theming]]) per explicit later request. Ordinary
      directories are *not* colored — checked against a real Far
      screenshot, only VCS dirs actually stood out there. Added
      `Theme::success`/`warning` (from the original "color 2" plan,
      only `danger` had actually landed before) to drive it
- [ ] `ColorScheme`'s `black`/`white`/most `bright*` fields are parsed
      but not yet consumed by `to_theme`/`to_syntax_theme` — kept for
      format fidelity and future mapping expansion, same situation as
      `Entry::size`/`modified` below; `#[allow(dead_code)]` on the
      struct silences the warning deliberately rather than dropping
      the fields
