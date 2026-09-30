# Popup chrome (`ui/popup.rs`) -- history

Code: `src/ui/popup.rs`. The corner/fill decision and its five rejected
attempts: `.claude/rules/litastum-popup-design.md`.

- **Padding.** Content first sat flush against the border
  (`Block::inner` alone) and read as cramped next to the reference
  mockup. `ratatui`'s recommended `+2` horizontal / `+1` vertical
  (visually equal, since cells are about twice as tall as wide) was
  tried first and reported uneven at the rounded corner, so it's
  `Padding::uniform(2)`: equal cell counts on every side.
- **`chrome_extra_rows`.** `Rounded` needs padding plus a title row
  beyond `Classic`'s border; a height formula tuned for `Classic` left
  `Rounded` no room for content.
- **`Classic` came back** as a permanent alternative (F9 -> Options ->
  UI), not something to migrate away from -- the square border with the
  title on it that every popup used before this module.
- **`percent_width`** for Find file's popup, reported too narrow on a
  wide terminal. Most popups stay a fixed width on purpose.
- **`selected_row_style`/`selected_text_style`.** The same
  `fg(theme.text).bg(theme.current_row_bg)` literal had spread to nearly
  every popup; when a scheme set a bright `current_row_bg` with
  `selectionForeground` (black on green), only the panel row honored it
  (`theming.md`).
- **`draw_list_popup`**: six popups (F9 menu, shell, drive, popup style,
  editor menu, editor keymap) had copied the same ~25 lines. Callers
  format their own labels -- generalizing that part would have needed
  more parameters than the duplication was worth.
