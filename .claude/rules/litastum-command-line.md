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
1. Modifier chords -- `Ctrl+O` (show/hide panels), `Ctrl+P` (shell
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
   (`Shift`+drag still selects in Windows Terminal); a shell command and
   `Ctrl+O` get the mouse back (`terminal_setup::release_mouse_capture`).
2. Selection and word moves in the command line (`Shift`/`Ctrl` +
   arrows); `Shift+Left/Right` select only while something is typed.
   `Ctrl+C`/`Ctrl+Insert` copy and `Ctrl+X`/`Shift+Delete` cut the
   selection, on a typed line only (above `Delete` = delete forward).
3. Marking -- `Shift+arrows`, and `Shift+A` on an empty line only.
4. The typed line -- `Enter` runs it; while history suggestions show,
   `Up`/`Down`/`Tab` work on them (never `Enter`), and `F8` forgets the
   highlighted one -- above `F8` = Delete, so it never deletes files
   then; `Tab` completes.
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
- **`cls`/`clear` are special-cased too**, the same way `cd` is —
  `submit_command_line` returns `Effect::ClearScreen` (a plain
  `terminal.clear()`) and never suspends the TUI at all, instead of actually shelling out. Found by
  running `cls` for real: a screen-clear command's entire job is
  leaving nothing on screen, so shelling out to a real `cls` wiped even
  the `"{cwd}> cls"` prompt line printed for every other command, and
  the "Press any key to continue..." pause (which exists to protect
  real command *output* from vanishing) ended up guarding nothing —
  just a stray message on an otherwise blank screen, reported as a
  broken-looking screen. `terminal.clear()` is what the command is
  actually trying to accomplish anyway, far more directly than a
  subprocess round-trip.
- **A command actually runs by suspending the TUI and inheriting
  stdio** (`Effect::RunShell` -> `run_shell_command_lines`) — not a captured/parsed
  output pane. This is deliberate: it's what makes interactive things
  (`python`, `git commit` invoking an editor, ...) work at all, and
  gives real colors/prompts, matching Far Manager's own behavior. A
  `Press any key to continue...` pause follows so fast-scrolling output
  isn't gone the instant the panels redraw over it.
- **litastum's two own printed lines** (the echoed `"{cwd}> {input}"`
  prompt and the `Press any key...` pause) are colored via
  `browsing.rs::print_themed` — `theme.text` on `theme.bg`, the closest
  match to real Far's own `CommandLine.UserScreen` color group
  (requested directly from a Far color-picker screenshot). **The
  shelled-out command's own output is never colored by us** — real
  inherited stdio means the terminal itself renders it, unlike Far's
  own full-screen text-mode architecture, which draws even a child
  process's output through its own buffer and can therefore recolor
  it. Reproducing that would need a PTY-based capture-and-recolor
  layer, well beyond this project's current inherit-stdio design.
  Approximated with `theme.text` rather than adding a dedicated
  `Theme` field for Far's exact `brightWhite` — close enough
  (`#cccccc` vs. `#f2f2f2` in `far-lts-alien.json`) that a whole new
  field for a two-line, rarely-focused-on piece of chrome wasn't
  judged worth it.

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
just fails and the OS error shows in the pause message — honest
feedback, no pre-flight probing needed either. See `TODO/command-line.md`.

**Not persisted** to `config.json` — resets to the platform default
each run. `config.rs` already has the pattern for persisting a choice
(from the theme picker) if this turns out to be wanted; left out here
to keep the change scoped to "pick a shell for this session," which is
what was actually asked for.

## Show/hide panels (`Ctrl+O`)

Real Far Manager's own toggle, requested directly to look at output
already sitting on the real terminal (something run through `Enter`/a
user-menu item, or anything printed before litastum even started)
without re-running whatever produced it. `browsing::toggle_panels_hidden`
is blocking and stateless — no `App` field anywhere records "panels
are currently hidden"; it simply doesn't return control to the main
loop's own `terminal.draw()` call until the panels should reappear,
the same shape `run_shell_command_lines` already uses for its own
"press any key to continue" pause.

Implementation: `LeaveAlternateScreen` (revealing the real terminal's
primary buffer, whatever's actually on it), then a blocking loop
reading raw `crossterm` events directly (bypassing the normal per-
frame `handle_event`/`draw` cycle entirely) until `Ctrl+O` is pressed
again — every other key and mouse event is silently ignored while
hidden, matching real Far's own behavior for this toggle, not just
this app's own scope cut. `EnterAlternateScreen` + `terminal.clear()`
on the way back out, same as `run_shell_command_lines`'s own return
path.

**Deliberately doesn't touch raw mode**, unlike `run_shell_command_lines`
(which disables it so a real subprocess gets normal line-buffered
input): there's no subprocess here to hand the terminal to, and
staying in raw mode means a stray keypress other than `Ctrl+O` is
silently swallowed rather than echoed as literal text onto the very
output the user is trying to look at cleanly. No "press any key"
message either, unlike that same function's own pause — the entire
point is showing exactly what's already there, not adding to it.

No unit test coverage for the hidden console itself: its loop reads
real `crossterm` events directly. Getting there (`Ctrl+O` ->
`Effect::ToggleHiddenPanels`) is covered by `handle_browsing_key`'s tests.

## Handlers return `Effect`, `event_loop` owns the terminal

Key handlers never take the `Terminal`. Anything that needs the real
console -- running shell lines, `cls`, the hidden console -- comes back
as a `command_line::Effect` (`RunShell`, `ClearScreen`,
`ToggleHiddenPanels`), and `event_loop::keys::dispatch_key_event`
performs it (`apply_effect`). `key_effect` is the whole key routing
without a terminal, so the browser's dispatch order, history recall and
F2 menu execution are all unit-tested. A new handler that needs the
console should add an `Effect` variant rather than take a `Terminal`.

**Every key handler has the same shape**: `fn(&mut App, KeyEvent) ->
Result<Effect>`, screens and overlays alike -- most return
`Effect::None`. So any handler can start asking for terminal work
without changing its signature or the dispatch in `key_effect`.
