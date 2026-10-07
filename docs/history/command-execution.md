# Running commands from the command line / user menu -- history

Current decisions: `.claude/rules/litastum-command-line.md`. Code:
`src/command_line/browsing/` (`shell_exec.rs`, `hidden_console.rs`,
`mod.rs`), `src/terminal_setup.rs::install_ctrl_c_handler`.

## Quoting on Windows

1. **`Command::arg` mangled quoted arguments.** `svn cleanup ...
   "Project Alpha"` failed with `Error resolving case of
   '"Project Alpha"'`. `Command::arg` re-escapes for
   `CommandLineToArgvW` (an extra pair of outer quotes, backslash before
   every inner quote), but `cmd /C` and `powershell -Command` re-parse
   their argument as a whole command line with their own rules, where a
   backslash doesn't escape a quote -- the user's quotes reached `svn`
   mangled. `append_command_line` uses `CommandExt::raw_arg` on Windows,
   so the shell sees exactly the typed text. On Unix `sh -c` gets one
   real argv element, so plain `.arg` is right.
2. **`cmd`'s leading-quote strip.** Once `resolve_app_paths_command`
   started rewriting a bare name to its App Paths location,
   `svn status "RFI15.0"` became `"C:\Program Files\SlikSvn\bin\svn.exe"
   status "RFI15.0"` and failed with `'C:\Program' is not recognized`.
   Per `cmd /?`, `/C` keeps quotes literally only when there are exactly
   two; otherwise, if the first character is a quote, it strips the
   first and last character -- here the quotes protecting the exe path.
   `wrap_leading_quote_for_cmd` adds one redundant outer pair for that
   strip to remove.

Both have regression tests that spawn a real `cmd.exe`. The first one
deliberately starts with a plain word (`call ...`), so the second quirk
can't give a false result for it.

## What the console shows around a command

- **`cls`/`clear` are handled in-process.** Shelling out wiped even the
  `"{cwd}> cls"` echo, leaving the "Press any key..." pause alone on a
  blank screen. `terminal.clear()` is what the command means anyway.
- **The echoed prompt is foreground-only.** Painting `theme.bg` behind
  it made a highlighted-looking box, because the terminal's default
  background isn't necessarily `theme.bg` (the same problem as the popup
  fill in `.claude/rules/litastum-popup-design.md`).
- **The "Press any key" pause after commands was dropped**: an extra
  keypress every time. `Ctrl+O` already covers looking at output again.
- **The prompt printed three times after one command.** Keys pressed
  while a child owned the console (an impatient `Enter` during a slow
  `svn merge`) queue at the OS level and were read afterward as empty
  command lines. `drain_stale_input` discards the queue right after raw
  mode comes back.

## `cd` inside multi-line user-menu items

A Far menu item `cd W:\WorkCopies\rust` + `cargo make diffs4` ran
`cargo make` in the original directory: each line is its own shell
process. Far runs item lines through its own command line, where `cd`
is Far's. `run_shell_command_lines` now handles `cd` lines itself
(moving the active panel), and a `cd` to a missing directory stops the
item -- running the rest in the wrong place is how `cd build` / `del *`
goes wrong. `parse_cd_target` also learned `cd /d` and quoted paths,
common in Far menus, which used to end up inside the path.

## `Ctrl+O` (hidden panels)

Requested as Far's show/hide-panels toggle, then extended into a real
command line while hidden (Far lets you keep typing there). Commands
run there stay on the console; only `Ctrl+O` returns.

Two bugs reported together against a slow `svn` merge:

1. **`Ctrl+O` sometimes did nothing.** Pressed while the child owned the
   console, it queued at the OS level -- and `drain_stale_input` threw it
   away with the stray `Enter`s. Unlike those, it carries intent: the
   drain now reports a queued `Ctrl+O` and the hidden loop honors it.
2. **`"path> path> svn st"`.** Fully deterministic: the hidden-mode
   prompt deliberately has no trailing newline, so leaving via `Ctrl+O`
   without pressing `Enter` left it dangling, and the next thing printed
   continued on that line. The loop now always ends with a newline.

## `Ctrl+C` killed litastum along with the child

Raw mode is what turns `Ctrl+C` into an ordinary key; with it off for a
child process, `Ctrl+C` is a real `CTRL_C_EVENT`/`SIGINT` for every
process on the console, and with no handler ours died too. An empty
handler (`ctrlc` crate, both platforms) keeps us alive; the child still
gets interrupted as it would in a real shell.

## Browser key bindings

- **`Shift+A` marks all only on an empty command line**: otherwise a
  command could never start with a capital letter.
- **`Ctrl+Shift+Left/Right` word selection** came from a real gap:
  fixing "go info" into "svn info" meant backspacing everything after
  the typo.
- **`Tab` completes while something is typed**: it used to switch
  panels even mid-command, backwards from every shell.
- **`Alt+F5` with exactly two marked entries** compares those two, added
  on top of the original "cursor file in each panel" convention.

## Resolving and recalling commands

- **`devenv` wasn't found** ("'devenv.exe' is not recognized"): Visual
  Studio registers itself only under the App Paths registry key, which
  Explorer and `ShellExecute` consult but `cmd.exe` doesn't. Bare names
  are resolved through it now. The first version still found nothing:
  the registry value is stored with quotes, which became part of the
  path until stripped.
- **`Enter` in the History popup runs the command**, reported as the
  expected behavior (like a shell's recall); copying it into the line
  to edit first moved to `Tab`.

## Our own user screen (pseudoconsole)

Requested: suggestions while the panels are hidden (`Ctrl+O`), drawn as
over the panels. With commands writing straight to the real terminal
(inherited stdio, the TUI suspended), nothing could be drawn over their
output without destroying it -- it can't be read back. A list printed
under the prompt was tried first (scrolling the output up, as
PowerShell's list view does) and replaced the same day by the real
fix, Far's own design: the user screen is ours.

- Each command runs in a pseudoconsole (`alacritty_terminal::tty`,
  ConPTY on Windows), its output parsed into a grid on a thread of its
  own (`user_screen/session.rs`), drawn live by `ui::draw_console` with
  our command line and key bar under it; keys are encoded for the
  program (`user_screen/keys.rs`). One pseudoconsole per command: a
  fresh grid each time, so ConPTY's absolute cursor moves never touch
  the previous commands' output, which is kept as lines
  (`UserScreen`).
- Commands are set apart by three blank lines (requested).
- After the program exits, ConPTY may still be drawing its last
  output: the reader keeps going until it's been quiet for 60 ms (at
  most 1 s).
- The quoting rules (`raw_arg`, the extra wrap for a leading quote)
  carry over: `alacritty_terminal` puts the shell's arguments on the
  command line unescaped. Their regression tests now run a real `cmd`
  in a pseudoconsole.
- The `Ctrl+C` handler above stays: harmless, though a program in its
  own pseudoconsole gets `Ctrl+C` there, not on our console.

