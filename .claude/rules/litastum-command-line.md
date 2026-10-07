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

Keys are resolved by one ordered table,
`command_line/browsing/bindings.rs::BINDINGS`: each row is a key, the
modifiers it needs and must not have, a condition (`Always`,
`EmptyLine`, `TypedLine`, `SuggestionsShowing`) and an action. The
first matching row wins; a key no row matches types into the command
line. Order matters and the table is grouped by it:
1. Modifier chords -- `Ctrl+O` (hide the panels), `Ctrl+P` (shell
   picker), `Ctrl+U`, `Shift+F6`, `Shift+Enter` on an empty line,
   `Alt+F1/F2/F5/F7/F8`, `Ctrl+L`/`Ctrl+F2`. They sit above the plain keys they
   would otherwise fall through to (`Alt+F5` above `F5` = Copy).
   `Ctrl+L`/`Ctrl+F2` turn the active panel's path title into a field, as in
   Compare (`browsing/panel_path.rs`); so does a click on it. While it's
   open it takes every key, ahead of the table, and the panel shows the
   directory the typed path is in (`Esc` takes it back); `Tab` completes the path,
   with a list to pick from when several entries match
   (`path_edit/complete.rs`, every path field). `F4` on a file in that
   list (or typed out) selects it in the panel and opens the editor. The click needs mouse
   capture in the browser, which takes the terminal's own text selection
   (`Shift`+drag still selects in Windows Terminal). With the panels
   hidden the wheel scrolls the user screen instead.
2. Selection and word moves in the command line (`Shift`/`Ctrl` +
   arrows); `Shift+Left/Right` select only while something is typed.
   `Ctrl+C`/`Ctrl+Insert` copy and `Ctrl+X`/`Shift+Delete` cut the
   selection, on a typed line only (above `Delete` = delete forward).
3. Marking -- `Shift+arrows`, and `Shift+A` on an empty line only.
4. The typed line -- `Enter` runs it; while suggestions show (history
   entries, then the active panel's names starting with the typed word,
   `command_line/suggestions.rs`), `Up`/`Down`/`Tab` work on them (never
   `Enter`) -- a history entry replaces the line, a name just the word --
   and `F8` forgets a highlighted history entry (above `F8` = Delete, so
   it never deletes files then; on a name it does nothing). `F4` on a
   file name selects it in the panel and opens the editor (otherwise it
   is plain `F4`); `Tab` completes.
5. Panel navigation and the F-key row, with any modifiers (these are
   below every chord on the same key) -- `Enter`/`Tab` here only on an
   empty line, since the rows above took the typed case.
6. `Esc`/`Backspace`/`Delete` edit the line.

`bindings.rs`'s tests pin the order (`a_chord_wins_over_the_plain_key_below_it`,
...); `handle_browsing_key`'s tests cover the resulting behavior.

## Scope cuts (deliberate, not oversights)

- **Bare arrows never move a cursor within the typed text** — they're
  needed for panel navigation even while something's typed, so they
  can't also mean "move within the command line" without real
  ambiguity. This is still true, but no longer means "append/backspace
  only": `Shift+Left`/`Right` and `Ctrl+Shift+Left`/`Right` (character-
  and word-wise) *do* select within the command line now — panel
  navigation never claimed those modifier combinations, only bare
  arrows. `App::command_line` is a `text_field::TextField`, the same
  type every other text field uses; `command_line/browsing`'s handling
  calls its selection/word methods directly rather than its full
  `apply_key` layout. Reported as a real gap: fixing a typo in the middle
  of a typed command (`"go info"` meant to be `"svn info"`) had no way
  to select and replace just the wrong word. Plain `Ctrl+Left`/`Right`
  (no `Shift`) came right after — cursor movement by a word with no
  selection, same underlying `text_field::move_word_left/right`.
- **`/` and `\` are their own word-movement stop**
  (`text_field::is_path_sep`), not chained into an adjacent word the
  way a space or `.` is — reported directly against a real URL/path
  (`/branches/Features/DataExtractionCDATree`): treating `/` like any
  other separator meant Ctrl+Right from right before one jumped
  straight through it *and* the whole next path segment in one press,
  so landing right after just the `/` needed bouncing Ctrl+Right then
  Ctrl+Left. Deliberately narrow — only these two characters, not every
  separator — so the established "skip a punctuation run, then the
  following word, in one press" behavior for spaces/dots/etc. is
  unchanged. Applies to both plain `Ctrl+Left`/`Right` and
  `Ctrl+Shift+Left`/`Right` selection, since both go through the same
  `move_word_left`/`move_word_right`.
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
  clean up `..`. `cmd.exe`'s own `cd /d <path>` form and a quoted path
  are understood too. **The same applies to every line of a multi-line
  F2 user-menu item** (`run_shell_command_lines`) — reported directly:
  `cd W:\WorkCopies\rust` followed by `cargo make diffs4` ran `cargo make`
  back in the original directory, since each line is its own shell
  process; Far runs a menu item's lines through its own command line,
  where `cd` is Far's own. A `cd` line now moves the active panel and
  every following line runs there; a `cd` to a missing directory stops
  the item instead of running the rest in the wrong place.
- **`cls`/`clear` are special-cased too**, the same way `cd` is:
  `submit_command_line` returns `Effect::ClearScreen`, which empties the
  user screen (below) and repaints -- no subprocess.
- **A command runs in a pseudoconsole, its output kept by us**
  (`Effect::RunShell` -> `run_shell_command_lines` -> `live_command::run_live`):
  ConPTY on Windows, a pty elsewhere, parsed into a grid by
  `alacritty_terminal` (`user_screen/session.rs`) -- the engine the
  window (`gui/`) uses. While it runs, the user screen shows the output
  live under our own command line, and every key goes to the
  program (`user_screen::encode_key`, `Ctrl+C` included), so interactive
  programs work. What's typed meanwhile also shows in our command line
  and stays there after the program ends unless it read it -- seen as
  its echo in the output; a line sent with `Enter` never comes back (a
  password prompt doesn't echo), nor do a full-screen program's keys
  (`browsing/type_ahead.rs`). Once the program echoes what's typed
  right before its cursor, it reads its input and gets the keys alone. When it exits, its output joins the user screen and
  the panels come back -- `Ctrl+O` shows it again. Replaced suspending
  the TUI with inherited stdio, so our own UI (popups, suggestions) can
  be drawn over the output, as in Far.
- **The user screen** (`App::user_screen`, `user_screen::UserScreen`)
  keeps what commands printed: each command as `"{cwd}> {line}"` in the
  prompt's colors, then its output, set apart from the previous one by
  three blank lines (requested). Capped at 20000 lines, oldest first.
  Colors in the output: the 16 ANSI colors by index (the real terminal's
  palette), the default colors as the terminal's own, RGB as is.

## Tab completion (`command_line::complete`)

Reported as a bug, not requested as a feature: `Tab` used to switch
panels unconditionally, even with text typed mid-command -- backwards
from every shell's own convention for the key. `Tab` now completes
while something is typed (a `TypedLine` row above the panel-switch
row); an empty line still switches panels.

Completes the *last whitespace-separated word* in the typed line as a
filesystem path relative to the active panel's directory (an
already-absolute word completes from its own root instead —
`Path::join`'s own behavior). One matching entry completes it fully,
with a trailing separator for a directory (so the next `Tab` continues
completing *inside* it) or a trailing space for a file (ready for the
next argument) — matching a normal shell's completion habit. No
matches leaves the line untouched, no bell or error shown. Matching is
case-insensitive.

**Several matches enter a `Tab`-cycling session**
(`App::command_line_completion: Option<command_line::CompletionCycle>`)
— the first `Tab` shows the first match (alphabetically), each further
`Tab` press steps to the next one, wrapping back to the first after the
last. This is `cmd.exe`'s own convention for the key, chosen explicitly
over completing to the matches' shared prefix and stopping there (an
earlier version of this did exactly that — replaced after being asked
for cycling specifically). Ending the session is `handle_browsing_key`'s
job, not `complete`'s: *any* other edit to the command line
(`insert_char`/`backspace`/`Esc`/running it/a bound command like a
panel move) sets `command_line_completion` back to `None`, so a stale
cycle never survives past the keystroke that should have ended it.

**Paths only, not command names** — no `PATH` scanning to complete
`car<Tab>` into `cargo`, same scope boundary as the existing `cd`
handling (which also only ever touches paths, not commands).

## Shell profile picker (`Ctrl+P`)

Windows Terminal's own "new tab" dropdown (PowerShell / Command Prompt
/ Azure Cloud Shell / Git Bash / ...) was the reference for this —
`shell.rs::ShellProfile` + `App::shell_profiles`/`active_shell`, a
`theme_menu.rs`-shaped popup (`Overlay::ShellMenu`, `Up`/`Down`/`Enter`/
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
just fails and the OS error shows on the user screen — honest
feedback, no pre-flight probing needed either. See `TODO/command-line.md`.

**Not persisted** to `config.json` — resets to the platform default
each run. `config.rs` already has the pattern for persisting a choice
(from the theme picker) if this turns out to be wanted; left out here
to keep the change scoped to "pick a shell for this session," which is
what was actually asked for.

## Show/hide panels (`Ctrl+O`)

Real Far Manager's own toggle: the user screen (above) with our own
command line and suggestions under it (`ui::draw_console`). A state of
the browser, `App::panels_hidden`, not a loop of its own: the main loop
draws the user screen instead of the panels, and keys go to
`browsing::hidden_console::console_key` -- so menus and popups open over
it as over the panels. `F2` (user menu), `F9` (menu), `F10` (quit) and
`Alt+F7` (Find file, from the active panel's directory) work there, as
requested; going to a Find file result shows the panels again. `Ctrl+F2`/`Ctrl+L`,
or a click on the prompt's path, edit that path in place, as a panel's
title (`panel_path`): the field covers the command line, its `Tab` list
opens upwards (`ui/path_edit.rs` puts it where there's room).

It's a real command line, as in Far: `Enter` runs the line there (its
output joins the screen live) and stays; `PageUp`/`PageDown` and the
mouse wheel scroll back; `Esc` clears the line; `Tab` completes a path;
arrows edit the line, the panels being hidden. The suggestions
(history, panel names) pop up over the line as over the panels, with
the same keys (`Up`/`Down`/`Tab`/`F4`/`F8`/`Esc`).

Text is selected there with the mouse (`user_screen/selection.rs`): a
drag selects, scrolling on at the top and bottom rows, the wheel moves
the selection's end with the text; or a click, scrolling, and a
`Shift`/`Ctrl`-click at the other end (`Ctrl` because Windows Terminal
keeps `Shift`-clicks for its own selection); a double click selects
the word (a path or URL stays whole), another double click on the
selected word the line; `Ctrl+C`/`Ctrl+Insert` copy it, `Esc`
drops it. Our own, since mouse capture takes the terminal's selection
-- and the alternate screen has no scrollback to select through.
History: docs/history/command-execution.md.

## Handlers return `Effect`, `event_loop` owns the terminal

Key handlers never take the `Terminal`. Anything that needs the real
console -- running shell lines, `cls` -- comes back as a
`command_line::Effect` (`RunShell`, `ClearScreen`), and `event_loop::keys::dispatch_key_event`
performs it (`apply_effect`). `key_effect` is the whole key routing
without a terminal, so the browser's dispatch order, history recall and
F2 menu execution are all unit-tested. A new handler that needs the
console should add an `Effect` variant rather than take a `Terminal`.

**Every key handler has the same shape**: `fn(&mut App, KeyEvent) ->
Result<Effect>`, screens and overlays alike -- most return
`Effect::None`. So any handler can start asking for terminal work
without changing its signature or the dispatch in `key_effect`.
