# Launching litastum in a window of its own -- history

Code: `src/bin/litastum-window.rs`, `terminal_setup::setup_terminal`
(`SetTitle`), `packaging/windows-terminal/litastum.json`.

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
