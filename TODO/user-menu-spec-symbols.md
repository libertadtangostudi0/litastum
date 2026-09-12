# User menu (F2): special symbols reference

Reference for every macro `explorer::user_menu::parse::substitution::substitute_macros`
understands in a `Commands` item's own command string(s) -- both real
Far Manager's own `!...!` family (its own `@MetaSymbols` help topic)
and litastum's own `{{...}}` family. See `TODO/user-menu.md` for the
feature's own history/design notes; this file is just the lookup
table.

## Far Manager (`!...!`)

Confirmed against Far's own `@MetaSymbols` help topic
(`FarEng.hlf.m4`), not guessed. A cell marked "self-terminating" has no
closing `!` of its own -- the macro ends the moment nothing more of it
matches, same as a real Far tokenizer.

| Symbol | Meaning | Notes |
|---|---|---|
| `!!` | Literal `!` | |
| `!` (nothing after) | Cursor file's name **without** extension | self-terminating |
| `!.!` | Cursor file's name **with** extension | |
| `` !` `` | Extension only | self-terminating |
| `!~` | Short name without extension | falls back to the long name -- no 8.3 short-name lookup in this codebase, and no such concept at all on macOS/Linux |
| `` !`~ `` | Short extension only | falls back to the long extension, same reason |
| `!-!` | Short name with extension | falls back to the long name |
| `!+!` | Same as `!-!`, but Far restores the long name if it was lost after running the command | falls back to the long name |
| `!&` / `!&Q` / `!&q` | Space-separated list of marked files (or just the cursor file if nothing's marked) | `Q` (default) quotes each name, `q` doesn't |
| `!&~` | Same list, short names | falls back to `!&` |
| `!@!` / `!$!` | "Name of a file containing the list" (Far writes the list to a scratch file, to dodge a command-line length limit) | falls back to the same inline list `!&` produces -- litastum doesn't write a scratch file here, to keep `parse` I/O-free |
| `!:` | Current drive (`C:`) or UNC share root | approximated as the path's own first component |
| `!\` | Current path | self-terminating |
| `!/` | Short current path | falls back to `!\` |
| `!=\` / `!=/` | Path with symbolic links resolved | falls back to the plain, unresolved path -- no canonicalization helper here yet |
| `!?!` | Description of the current file (Far's own `descript.ion` files feature) | unsupported, left as literal text -- litastum has no per-file description feature |
| `!?Label?Default!` | Interactive prompt before running -- one popup per unique `Label`, pre-filled with `Default` | `Mode::UserMenuPrompt` |
| `!##` | Switch every macro *after this point in the same command* to the **passive** panel | stays in effect until the next prefix |
| `!^` | Switch to the **active** panel | the default for a fresh command |
| `![` | Switch to the **left** panel | independent of which panel is active |
| `!]` | Switch to the **right** panel | independent of which panel is active |

## litastum-native (`{{...}}`)

Picked specifically to collide with nothing cmd.exe (`%VAR%`, or
`!VAR!` under delayed expansion -- the exact same delimiter Far's own
`!...!` uses, which is the real problem this syntax avoids), PowerShell
(`$var`, `${...}`), or POSIX `sh` (`$var`, `$(...)`, backticks) already
gives special meaning to.

| Symbol | Far equivalent | Example |
|---|---|---|
| `{{cursor}}` | `!.!` | `type {{cursor}}` -> `type report.txt` |
| `{{prompt:Label}}` | `!?Label?!` | `git checkout {{prompt:Branch}}` -- asks for "Branch", substitutes the answer |
| `{{prompt:Label:Default}}` | `!?Label?Default!` | `git checkout {{prompt:Branch:main}}` -- the input field starts pre-filled with `main` |

Both prompt syntaxes (`!?...?!` and `{{prompt:...}}`) can be mixed in
the same command -- both go through the exact same `Mode::UserMenuPrompt`
popup.

Currently just these two macros -- more litastum-native equivalents
(marked list, current path, ...) can follow the same
`consume_litastum_token` pattern later if ever wanted (see
`TODO/user-menu.md`'s own gaps section).
