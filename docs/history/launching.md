# Launching litastum in a window of its own -- history

Code: `gui/` (the window), `xtask/` (`cargo xtask dist`),
`terminal_setup::setup_terminal` (`SetTitle`),
`packaging/windows-terminal/litastum.json`. The first launcher,
`src/bin/litastum-window.rs`, is gone (stage 3 below).

## Why litastum opens in Windows Terminal

litastum is a console program: Windows starts any console `.exe` in the
default terminal host (Windows Terminal on Windows 11, conhost before).
Asked whether it could be self-contained instead. Three ways were laid
out:

1. **Stay a console app, launch it into a window of its own** -- chosen.
2. Bundle a portable terminal (WezTerm, Alacritty) with a config --
   tens of megabytes of someone else's program.
3. A GUI backend for ratatui (winit + wgpu). Rendering would carry over,
   but input moves off crossterm, running shell commands and `Ctrl+O`
   need a real console -- an embedded ConPTY terminal emulator -- and
   the "runs in any terminal / over SSH" property goes, the same reason
   Slint was rejected (`.claude/rules/litastum-stack.md`).

## What landed

- **`SetTitle("litastum")`** on startup, so the window or tab is named
  after the app rather than the exe path.
- **`litastum-window.exe`**, a second binary built next to
  `litastum.exe`: a Windows GUI-subsystem program (no console flashes up
  when it's double-clicked) that runs `wt -w new new-tab --title
  litastum -d <cwd> litastum.exe` -- always a new window, never a tab in
  an open one -- and falls back to `CREATE_NEW_CONSOLE` (a conhost
  window) where Windows Terminal isn't installed. Not `--focus`: focus
  mode hides the title too. Window size is left to the user's settings.
- **A Windows Terminal fragment** (`packaging/windows-terminal/`): a
  `litastum` profile with its own color scheme (the bundled default,
  `themes/github-dark-default.json`) and font. Nothing installs it yet:
  fragments go under `%LOCALAPPDATA%`, which stays untouched until the
  installer exists (`.claude/rules/litastum-config.md`); the profile's
  `commandline` assumes the installer puts litastum in
  `%LOCALAPPDATA%\Programs\litastum\`. Once installed, a shortcut can
  run `wt -w new -p litastum`.

## Then: a window of its own (`gui/`)

Opening into Windows Terminal still read as "it runs in the terminal".
Far looked self-contained because it ran in a classic console window;
asked for a real window by default that still runs in any terminal.

Two ways were weighed:
- **A. A window backend for ratatui** in the app itself: input moves off
  crossterm everywhere, and running shell commands and `Ctrl+O` would
  need an embedded pseudoconsole and terminal parser anyway.
- **B. A small terminal emulator hosting the unchanged console app** --
  chosen. The console app keeps a real console (ConPTY), so commands,
  their output and `Ctrl+O` work as they do in any terminal; the
  pseudoconsole and the parser exist once, in the host.

Slint was raised again (it has a markup language, which `winit` and
`egui` don't). It still has no character grid -- the screen would be
one image drawn by our own code -- so its markup would only pay off for
real GUI elements around the terminal (tabs, toolbars, dialogs), which
aren't wanted. `egui` was ruled out outright.

The host (`gui/`, workspace member `litastum-gui`): `winit` for the
window and input, `softbuffer` + `cosmic-text` (swash) drawing cells on
the CPU, `alacritty_terminal` (also behind Zed's terminal) for the
pseudoconsole and the screen state -- all pure Rust. Every glyph is
shaped once and placed by column, never by a shaped line's advances, so
a fallback glyph can't shift the row. The default colors are the bundled
default theme. Keys go to the console app as xterm sequences (`keys.rs`),
which ConPTY turns back into console key events.

Stages: 1. the window with litastum running in it (this); 2. full input
(`win32-input-mode` for combinations xterm can't express, mouse, paste,
DPI changes); 3. shipping as `litastum.exe` (window) + `litastum.com`
(console, preferred by `cmd` for a bare `litastum`), replacing
`litastum-window.exe`; 4. looks (theme colors, cursor, bold/italic);
5. images in F3.

### Stage 2: input

- **Keys as `win32-input-mode` records on Windows** (`win32_input.rs`):
  every press and release goes to ConPTY as
  `ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc _`, which it turns into the exact
  `KEY_EVENT` a real console delivers -- what Windows Terminal sends.
  xterm sequences (`keys.rs`, still the Unix path) can't carry
  `Ctrl+Shift+Z` apart from `Ctrl+Z`, nor key releases. `Ctrl+letter`
  goes by the physical key, so it works on the Russian layout.
- **Paste as Windows Terminal does it**: `Ctrl+V`/`Shift+Insert` never
  reach litastum as keys; the window types the clipboard's text in.
  litastum already reads the clipboard itself on the physical `Ctrl+V`
  and swallows that typed copy (`windows_terminal::paste_hotkey`,
  `PasteFlood`), so both hosts behave the same.
- **Mouse** reports (`mouse.rs`): clicks, drags and the wheel as SGR
  (`ESC [ < b ; x ; y M/m`), only while the program asked for them --
  ConPTY asks while litastum has mouse capture on.
- **DPI**: moving to a monitor with another scale rebuilds the font at
  that size and resizes the pseudoconsole even when the grid stays the
  same (the cell's pixel size changed).
- **Even side margins** (reported with a screenshot): the width that
  doesn't fill a whole cell all sat on the right. The grid is centered
  now (`render::grid_origin`); the mouse maps through the same origin.
  A border's `│` still sits mid-cell, as in any terminal.
- **A dark window frame** (reported: the title bar was the system's
  yellow accent above the dark grid): the window asks for the dark
  theme, and on Windows 11 its title bar, title text and border take the
  terminal's background and foreground colors.

### Stage 3: shipping names

`cargo xtask dist [--release]` builds the two programs and lays them out
in `dist/` (git-ignored): on Windows the window becomes `litastum.exe`
and the console app `litastum.com`. A double click or a shortcut opens
the window; `litastum` typed in cmd, PowerShell or Far runs the console
twin in that terminal, since `.COM` precedes `.EXE` in `PATHEXT` --
Visual Studio's `devenv.exe`/`devenv.com` trick; a `.com` here is an
ordinary executable. In `target/` nothing is renamed, so
`cargo test --bin litastum` and `cargo run` are unchanged; the window
looks for `litastum.com`, then `litastum.exe`, never itself.

- The window and taskbar icon is drawn in code (`gui/src/icon.rs`): two
  panels with a highlighted row, in the default theme's colors. The exe
  file's own icon in Explorer needs a compiled `.ico` resource -- left
  for when there's a real icon design.
- `litastum-window.exe` is removed: the window replaces it. The Windows
  Terminal fragment now points at `litastum.com`, the console app.

### Stage 4: looks

- **Theme colors through the terminal**: litastum sends its theme's
  text, background and cursor colors as `OSC 10`/`11`/`12` whenever the
  theme changes, and resets them on exit (`terminal_palette.rs`). The
  window reads them like any terminal would -- margins, unpainted cells
  and the Windows 11 title bar follow -- and Windows Terminal picks them
  up too. Reading `config.json` in the window instead would have been a
  second copy of the theming code.
- **Cursor blinking** for a blinking cursor style (litastum's bar):
  530 ms halves, steady while typing.
- **Bold and italic** from the font's own faces (glyphs cached per
  style), **dim** text mixed toward the background, **underlines** and
  **strikeout** as one-pixel lines.

### Stage 5: images in F3

Probed ConPTY directly (a process printing each protocol through it):
it passes iTerm2 inline images (`OSC 1337 ; File=`) through untouched,
but drops Sixel (`DCS`) and kitty (`APC`) -- so iTerm2 it is. It also
answers `CSI c`, `5n` and `6n` itself, and forwards `CSI 14t`/`16t`.

- **The window cuts images out of the stream** (`intercept.rs`): its
  own pty I/O thread replaced `alacritty_terminal`'s event loop, so the
  bytes are seen before parsing and an image lands at the cursor cell it
  arrived at. The PNG is decoded (`images.rs`) and drawn over its cells;
  it goes once text is written into any of them (`ratatui-image` clears
  the area first, then skips those cells, so text means the preview
  closed or the screen moved on) or the grid is resized.
- **litastum is told, not asked** (`image_host.rs`): `ratatui-image`'s
  query ends on the `CSI 5n` reply, which ConPTY gives before the
  window's cell-size reply can arrive, so it fell back to half-blocks.
  The window sets `LITASTUM_HOST_CELL_SIZE` (`"10x20"`) for the console
  app, which then picks iTerm2 at that cell size without querying.
  `Picker::from_fontsize` is deprecated in favor of the query that can't
  work here.
- The window answers `CSI 14t` (text area in pixels) and `OSC 10/11/12
  ; ?` color queries itself, which `alacritty_terminal`'s loop used to.

The cell size is passed once, at start: after a DPI change images are
still sized for the old cells until litastum restarts.

## Review pass: refactor, bugs, performance

`main.rs` was split (`app`, `input`, `window_style`), and a review
turned up:

- **Touchpad scrolling did nothing**: pixel deltas, and fractional
  lines from fine-grained wheels, were rounded per event, to 0.
  `input::WheelAccumulator` carries the remainder.
- **The wheel did nothing in the panels**: without mouse reports, on the
  alternate screen it now sends arrow keys, as Windows Terminal and
  xterm's alternate scroll mode do (`input::alternate_scroll`).
- **Emoji typed in were broken**: a console key record carries one
  UTF-16 unit; text outside the BMP now goes as plain UTF-8.
- **IME input (Win+., CJK) was lost**: `Ime::Commit` is written as text.
- **Focus was never reported** though ConPTY asks for it (`?1004h`).
- **An oversized image printed its base64 as text** once dropped
  mid-stream; it's now skipped to its terminator.
- **An image whose `ESC \` terminator was split between two reads never
  ended** -- the `ESC` went into the body. Found by a new test; the cut
  `ESC` is held for the next read.
- **Up to 16 ms of lag per key after a quiet spell**: input didn't reset
  the I/O thread's polling backoff.

**Performance**: every frame redrew every cell and glyph, a glyph pixel
at a time through a closure. `render::Renderer` keeps a back buffer and
redraws only what changed -- the terminal's damage, the cursor's old
and new line, the lines under images -- and glyphs are cached as
coverage masks (`font::GlyphMask`) drawn by a plain loop. Measured on a
200x60 grid full of text: a full redraw 4.3 ms (release) / 107 ms
(debug); typing one character 0.04 ms / 0.8 ms. The I/O thread also
locks the terminal once per read and hands text over without copying.
