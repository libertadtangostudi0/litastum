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

Dispatch order in `command_line.rs::handle_browsing_key` (moved here
from `main.rs` when every mode's handling got split out of it — this
exact order matters, each step only runs if the previous one didn't
already handle the key):
1. `Ctrl+P` → open the shell picker (below).
2. `Shift+F6` → rename prompt (needs the raw modifier, same reason as
   Tab below — `keymap::resolve`'s table only keys off `KeyCode`).
3. `Enter` with a non-empty command line → run it.
4. `Tab` with a non-empty command line → complete it (below) instead
   of falling through to `keymap::resolve`'s Tab-as-`ToggleActive`
   binding.
5. `keymap::resolve` — the fixed table (arrows, Tab *on an empty
   line*, F4/F9/F10, and `Enter` on an *empty* line).
6. Anything left over (a plain character, `Backspace`, `Esc`) edits the
   command line (`command_line.rs`).

## Scope cuts (deliberate, not oversights)

- **Bare arrows never move a cursor within the typed text** — they're
  needed for panel navigation even while something's typed, so they
  can't also mean "move within the command line" without real
  ambiguity. This is still true, but no longer means "append/backspace
  only": `Shift+Left`/`Right` and `Ctrl+Shift+Left`/`Right` (character-
  and word-wise) *do* select within the command line now — panel
  navigation never claimed those modifier combinations, only bare
  arrows. `App::command_line_cursor`/`command_line_selection_anchor`
  plus `command_line/browsing.rs`'s handling directly reuse
  `text_field.rs`'s selection functions (built first for the Copy/Move
  destination field) rather than duplicating that logic — including two
  new word-wise ones, `extend_selection_word_left`/`_right`, added
  alongside this. Reported as a real gap: fixing a typo in the middle
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
  clean up `..`.
- **`cls`/`clear` are special-cased too**, the same way `cd` is —
  `run_command_line` calls `terminal.clear()` directly and never
  suspends the TUI at all, instead of actually shelling out. Found by
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
  stdio** (`command_line.rs::run_command_line`) — not a captured/parsed
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

Reported as a bug, not requested as a feature: `Tab` used to hit
`keymap::resolve`'s Tab-as-`ToggleActive` binding unconditionally, even
with text typed and mid-command — backwards from every shell's own
convention for the key. Fixed by special-casing `Tab` ahead of that
table (same pattern as `Ctrl+P`/`Shift+F6` above) whenever
`app.command_line` isn't empty; an empty line still falls through to
the panel-switch binding, so Tab's original behavior survives outside
the command line.

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
