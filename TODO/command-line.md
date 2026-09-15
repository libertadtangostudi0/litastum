# Command line (`command_line.rs`, `shell.rs`) — landed, gaps left

Always-live Far Manager-style command line with `cd` special-casing and
a `Ctrl+P` shell-profile picker (PowerShell/Command Prompt today). See
[[litastum-command-line]] for the full design and every scope cut below
in more detail.

- [ ] No cursor movement within the typed command (append/backspace
      only) — arrows are needed for panel navigation even while typing,
      so they can't double as text-cursor movement without real
      ambiguity
- [x] Text selection in the command line — `Shift+Left`/`Right`
      (character-wise) and `Ctrl+Shift+Left`/`Right` (word-wise) select,
      typing or `Backspace` over a selection replaces/deletes it, same
      shape as `text_field.rs`'s Copy/Move destination-field selection
      (which it directly reuses: `extend_selection_left/right`,
      the two new `extend_selection_word_left/right`, `delete_selection`)
      against a real cursor position the command line never had before
      (`App::command_line_cursor`/`command_line_selection_anchor`).
      Bare `Left`/`Right` are still panel-navigation-only, untouched —
      only the `Shift`/`Ctrl+Shift` combinations, which panel nav never
      claimed. Rendered with `theme.current_row_bg`, the same color the
      destination field's own selection already uses (not a distinct
      "Selected text" color reproducing Far's own olive-green group —
      judged not worth a dedicated `Theme` field for this one highlight,
      reusing the one that's already there for the same *kind* of
      selection elsewhere).
- [x] **`Ctrl+O` -- show/hide panels**, real Far Manager's own toggle,
      requested directly. `handle_browsing_key` (checked ahead of even
      `Ctrl+P`) calls `toggle_panels_hidden`, which leaves the
      alternate screen (revealing whatever's actually on the real
      terminal -- prior `run_shell_command_lines` output, or anything
      printed before litastum even started) and blocks reading raw
      `crossterm` events until `Ctrl+O` is pressed again, at which
      point it re-enters the alternate screen and `terminal.clear()`s
      before returning. No `App` field records "hidden" anywhere --
      same stateless, blocking shape `run_shell_command_lines` itself
      already uses for its own "press any key to continue" pause.
      Deliberately doesn't touch raw mode (unlike
      `run_shell_command_lines`, which disables it for a real
      subprocess) -- staying in raw mode means every other key is
      silently swallowed instead of echoing onto the very output the
      user is trying to look at cleanly, and there's no subprocess here
      to hand the terminal to in the first place. No "press any key"
      message either -- the whole point is showing exactly what's
      already there, not adding to it.
- [x] **`Ctrl+U` -- swap panels**, real Far Manager's own binding,
      requested directly. `App::swap_panels` (`app.rs`) swaps the two
      `Panel` structs in place (`self.panels.swap(0, 1)`) -- this alone
      carries over everything Far's own swap does (path, entries,
      cursor, scroll, marks), since the whole struct moves at once;
      `columns`/`visible_rows` come along too, but harmlessly, since
      both are recomputed from the panel's on-screen position every
      frame regardless of which panel index now holds which content.
      `self.active` is deliberately left untouched, so keyboard focus
      stays on the same screen *side* -- distinct from `toggle_active`
      (`Tab`), which moves focus without touching either panel's
      contents. Needs the raw `Ctrl` modifier, same reason as `Ctrl+O`/
      `Ctrl+P` above -- `keymap::resolve`'s table only keys off
      `KeyCode` -- so it's a new `Command::SwapPanels`, special-cased in
      `handle_browsing_key` right alongside those two.
- [x] Plain `Ctrl+Left`/`Right` (no `Shift`) moves the command-line
      cursor by a word with no selection — `text_field::move_word_left/
      right`, clearing any active selection rather than collapsing to
      its edge (a real editor's Ctrl+arrow moves from the cursor and
      drops the selection, it doesn't jump to whichever edge is
      closer). Added right after the Shift-selection work above, once
      it was pointed out plain Ctrl+arrow cursor movement (no
      selecting) was still missing — `resolve(key.code)` only sees
      `KeyCode`, not modifiers, so before this Ctrl+Left/Right silently
      fell through to the exact same panel-navigation move as a bare
      arrow (not a behavior loss — bare arrows already did that).
- [ ] No `Up`-arrow history recall — same reason, arrows are taken; see
      [history.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/history.md) for the menu-driven way around this
      instead. Requested
      directly: `Alt+Up`/`Alt+Down` to cycle through `app.command_history`
      while typing (bare arrows stay panel-navigation-only, unaffected) —
      `Alt` isn't otherwise claimed on these two keys, so this doesn't
      need the same workaround `Shift+F6`/`Ctrl+P` use to jump ahead of
      `keymap::resolve`'s table. Needs deciding how this interacts with
      the existing `Tab`-completion cycle (`App::command_line_completion`),
      which also reads `command_history` for its own separate cycling
      session.
- [ ] Bare `cd` (no argument) is a no-op, not "go to home directory"
- [x] `Tab` completes the last typed word as a filesystem path
      (`command_line::complete`) instead of always switching panels —
      reported as a bug: `Tab` used to hit `keymap::resolve`'s
      Tab-as-`ToggleActive` binding unconditionally, even mid-command,
      which is backwards from every shell's own convention for the key.
      Now special-cased ahead of that table (same pattern as `Ctrl+P`)
      whenever the command line has something typed; falls through to
      the usual panel-switch once it's empty. One match completes fully
      (trailing separator for a directory, trailing space for a file);
      several matches enter a `Tab`-cycling session
      (`App::command_line_completion`, a `command_line::CompletionCycle`)
      — each further `Tab` steps to the next match, wrapping back to
      the first after the last, `cmd.exe`'s own convention (explicitly
      requested over completing to the matches' shared prefix and
      stopping there); any other edit to the line ends the session.
      None leaves the line untouched. No completion for command *names*
      themselves (`PATH` scanning), only paths — same scope as the
      existing `cd` handling
- [x] `cls`/`clear` are special-cased like `cd` — `terminal.clear()`
      directly, no subprocess, no TUI suspend at all. Found by actually
      running `cls`: shelling out to a real `cls` wiped the `"{cwd}>
      cls"` prompt line we print for every command, leaving just the
      "Press any key to continue..." pause floating on an otherwise
      blank screen — technically working as designed, but looked like
      a broken/blank screen, reported as one
- [ ] Shell profile picker has no Git Bash/WSL/pwsh/Azure Cloud Shell
      entries (unlike the Windows Terminal dropdown that prompted this
      feature) — only `cmd`/`powershell` (Windows) or `$SHELL`/`sh`
      (Unix), which are universally present so need no detection.
      Adding the others needs real `PATH`/install-dir probing, not done
- [ ] Shell profile choice isn't persisted to `config.json` — resets to
      the platform default every run (`config.rs` already has the
      persistence pattern from the theme picker, if this is wanted)
- [ ] Opening the editor (F4) or another popup (F9, `Ctrl+P` itself)
      while text sits in the command line doesn't warn about it — the
      text is preserved and still there afterward, just easy to forget
      about since nothing currently calls it out
- [ ] No persistent, resizable console/output area below the panels —
      real Far Manager keeps one, adjustable with `Ctrl+Up`/`Ctrl+Down`
      (shrinks/grows the panel area to reveal more or less of it) and
      scrollable on its own with `Ctrl+Shift+Up`/`Ctrl+Shift+Down`.
      Reported directly against the current design's real limitation:
      one command's output replaces the previous one's the instant the
      next command runs (full TUI takeover + "Press any key to
      continue..." pause, see `print_themed`'s own doc comment in
      `command_line/browsing.rs`) — there's no on-screen history of
      what earlier commands printed at all, let alone a resizable or
      scrollable view onto it. This needs real output *capture* (a PTY
      or at least a piped/buffered child process, keeping a scrollback
      buffer in `App`) instead of today's "suspend the TUI, inherit
      stdio directly, let the terminal itself render it" approach — the
      same architectural gap already called out for why the shelled
      command's own output can't be recolored either. A genuinely
      bigger redesign than the other command-line gaps above, not a
      small addition on top of the current model.
