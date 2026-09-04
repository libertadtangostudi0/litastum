# litastum: command line

## Always live, Far Manager-style

The row under the panels (`ui.rs::draw_command_line`) isn't a separate
input mode you focus — it's always accepting text while browsing,
exactly like Far Manager's own command line: arrow keys still move the
panel selection, plain characters type into `app.command_line`, `Enter`
runs it (or does the usual `EnterSelected` if the line is empty).

**This is why bare `q` no longer quits** (`keymap.rs`) — once letters
type into the command line, a lone `q` has to be the start of a typed
command, not a shortcut. Only `F10` quits now, matching real Far.

Dispatch order in `main.rs::handle_browsing_key` (this exact order
matters — each step only runs if the previous one didn't already
handle the key):
1. `Ctrl+P` → open the shell picker (below).
2. `Enter` with a non-empty command line → run it.
3. `keymap::resolve` — the fixed table (arrows, Tab, F4/F9/F10, and
   `Enter` on an *empty* line).
4. Anything left over (a plain character, `Backspace`, `Esc`) edits the
   command line (`command_line.rs`).

## Scope cuts (deliberate, not oversights)

- **Append/backspace-only editing** — no left/right cursor movement
  within the typed text. Arrows are needed for panel navigation even
  while something's typed, so they can't also mean "move within the
  command line" without real ambiguity. Fixing a typo means
  backspacing to it.
- **No command history** (no up-arrow recall) — same reason, arrows are
  taken.
- **Bare `cd`** (no argument) is a no-op, not "go to home directory".
- **`cd` is special-cased**, everything else is shelled out. A spawned
  shell's own `cd` would only change *its* directory, not our
  process's or the panel's — so `command_line::parse_cd_target` catches
  it first and `Panel::change_dir` updates the panel directly, no
  subprocess involved. The resolved path is lexically normalized
  (`panel.rs::lexically_normalize` — `..` segments collapsed) rather
  than left as `.../sub/..`; deliberately *not* `fs::canonicalize`,
  which also resolves symlinks and — on Windows — prepends the ugly
  `\\?\` extended-length prefix, neither of which is wanted just to
  clean up `..`.
- **A command actually runs by suspending the TUI and inheriting
  stdio** (`main.rs::run_command_line`) — not a captured/parsed output
  pane. This is deliberate: it's what makes interactive things
  (`python`, `git commit` invoking an editor, ...) work at all, and
  gives real colors/prompts, matching Far Manager's own behavior. A
  `Press any key to continue...` pause follows so fast-scrolling output
  isn't gone the instant the panels redraw over it.

## Shell profile picker (`Ctrl+P`)

Windows Terminal's own "new tab" dropdown (PowerShell / Command Prompt
/ Azure Cloud Shell / Git Bash / ...) was the reference for this —
`shell.rs::ShellProfile` + `App::shell_profiles`/`active_shell`, a
`theme_menu.rs`-shaped popup (`Mode::ShellMenu`, `Up`/`Down`/`Enter`/
`Esc`). The active profile's name shows at the command line's right
edge, since which one a typed command runs against is otherwise
invisible.

**Built-in profiles only, no detection**: Windows ships `cmd` +
`powershell`; Unix gets `$SHELL` (or `sh`). Git Bash / WSL / pwsh /
Azure Cloud Shell — the extra entries in the Windows Terminal
screenshot that prompted this — are **not** included: their paths
aren't universal (Git Bash varies by install, WSL needs a distro,
pwsh may not be installed), and properly detecting them (`PATH`
scanning, common install dirs) is real, separate work, not done here.
If a user picks a profile that isn't actually installed, spawning it
just fails and the OS error shows in the pause message — honest
feedback, no pre-flight probing needed either. See `TODO.md`.

**Not persisted** to `config.json` — resets to the platform default
each run. `config.rs` already has the pattern for persisting a choice
(from the theme picker) if this turns out to be wanted; left out here
to keep the change scoped to "pick a shell for this session," which is
what was actually asked for.
