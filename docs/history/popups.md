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

## Individual popups

- **Command history**: grew with the match count (overflowing the
  screen) and didn't scroll -- the selected entry vanished below the
  border. Now fixed-height with a `ListState`, 70 columns instead of 60
  (long `svn`/`git` commands were clipped). It had also missed the
  `draw_frame` migration, so `Rounded` did nothing for it.
- **Find file**: the same no-scroll and growing-height problems with
  thousands of results; height 21 to match the color-scheme picker, as
  requested. The `Typing` phase, sized for `Classic`, rendered blank
  under `Rounded` -- which is what prompted `chrome_extra_rows`.
  Separators under the title and before the hints were requested from a
  screenshot of the picker.
- **The `Ctrl+F` box** first used the full card (dot, title, badge) and
  was reported too big and too labeled; now a one-row field at the
  editor's top-right, like VS Code.
- **The command line's shell-profile hint** (`Ctrl+P cmd` on the right)
  was removed as clutter.

## The F-key bar

- One flowing line reflowed every key from F7 on when `Alt` switched
  the labels (`"Folder"` vs. `"Find"`); now ten fixed columns.
- `"Bookmarks"` ran into `"F3 View"` -- columns have no gap, so labels
  stay within Far's 6 characters, enforced by a test.
