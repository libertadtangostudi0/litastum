# Editor performance: paste and search -- history

Current decisions: `.claude/rules/litastum-performance.md`. Code:
`src/editor/editor/fast_paste.rs`, `src/windows_terminal/`
(`paste_hotkey.rs`, `paste_flood.rs`), `src/event_loop/paste.rs`,
`src/event_loop/keys.rs::drain_pending_editor_typing`,
`src/editor/editor/search/session.rs`, `src/editor/editor/view.rs`.

Every round below was a real, reported symptom on a real file. The
recurring lesson: several plausible theories in a row, each backed by
real source reading, each wrong about the *reported* symptom -- only
re-measuring against the real scenario told them apart.

## Pasting felt "like watching it render line by line"

Pasting a real ~20KB / 338-line file (`ARCHITECTURE.md`) into the editor
took well over a minute; a larger paste sometimes meant several
unresponsive minutes.

### Round 1 -- `edtui`'s own paste algorithm (a real bug, not this one)

From source (`edtui` 0.11.7, `actions/cpaste.rs::paste` ->
`helper::insert_str` -> `insert_char`): pasted text is inserted one
character at a time, each a `Vec::insert` at a growing column -- O(n^2)
for a long single-line paste. `edtui-jagged`'s `Jagged` has no bulk
"splice into a row" API (only a single-element insert or a whole-row
insert). Patching the third-party crate wasn't an option without real
review, so `fast_paste.rs::splice_paste` does it locally with
`Vec::splice` on the row (`state.lines` and `Jagged::get_mut`/`insert`
are public), O(pasted length + line length).

Benchmarked against the reported file: the splice took under 1ms. Not
the reported bug -- kept for the genuinely pathological case (one huge
line: a JSON blob, an escaped log line).

It also cost `edtui`'s undo integration: `EditorState::capture()` is
`pub(crate)`, so a splice can't record a checkpoint. See
`editor-undo.md` for where that led.

### Round 2 -- one redraw per pasted character (right on Unix only)

`event_loop::run` redraws once per event, and without bracketed paste
every pasted character arrives as its own `Event::Key`. Enabled
`EnableBracketedPaste` and handled `Event::Paste` as one block. But
`crossterm` 0.29's Windows backend (`event/sys/windows/`) never
produces `Event::Paste`: bracketed paste there is built on parsing raw
ANSI bytes from stdin, and the Windows backend reads structured
`KEY_EVENT_RECORD`s via `ReadConsoleInputW`. Dead code on the reporting
platform. Kept (correct on Unix), along with
`drain_pending_editor_typing`, built alongside it: coalescing already
queued plain characters into one `paste_text` call is a real
improvement for any burst -- but characters that trickle in one at a
time are never queued in advance to coalesce.

### Round 3 -- the actual cause, from the log's timestamps

Consecutive `editor key` debug lines in `logs/litastum.log` were
**~7-8ms apart, like a metronome** -- the arrival rate itself. Windows
Terminal binds `Ctrl+V` to its own paste action and consumes the key
(the log never showed a `Ctrl+V` `KeyEvent` during a paste), then
injects the clipboard as simulated keystrokes at that throttled rate.
Nothing in this app's own redraw or insert path could fix that.

**Fix**: poll the physical `Ctrl+V` with `GetAsyncKeyState` (the
technique `alt_key.rs` already used for `Alt`), read the clipboard
directly and paste at once. Polled on every loop iteration, not only
when idle -- during a flood the loop is never idle. The flood still
arrives afterward (it can't be stopped), so it's swallowed
(`PasteFlood`).

### Follow-up bugs in the flood swallow

1. **`Ctrl+S`/`Ctrl+Z` right after a paste were eaten** -- the swallow
   keyed off `KeyCode` alone and ignored modifiers. The flood only ever
   sends unmodified characters.
2. **`Enter` pressed right after a paste registered 5-10 seconds late.**
   The swallow counted keys by *shape* (plain `Char` or bare `Enter`)
   against a decrementing counter, so real keystrokes typed during the
   window matched too. Now it matches the actual next expected
   character, and a mismatch ends the swallow.
3. **Pasting again quickly left slow, visible typing afterward.** The
   second paste *overwrote* the expected queue, so the rest of the
   first flood no longer matched and got typed. The queue is now
   extended; Windows Terminal delivers one paste's flood before the
   next. The deadline is recomputed over the whole queue.
4. **A terminal that passes `Ctrl+V` through** (instead of owning it)
   would have pasted twice: once through the bypass, once through the
   real key event. A real `Ctrl+V` event inside the swallow window is
   dropped.
5. **"Тесты добавлены:" pasted as "ТТесты добавлены:есты добавл..."** --
   a race: the flood starts the instant `Ctrl+V` goes down, and its
   first character can be typed normally before the next poll sees the
   physical key. The bypass then pasted the whole clipboard (doubling
   "Т") and expected a flood starting with "Т"; the flood's actual
   next character didn't match and the rest was typed. `PasteFlood`
   now remembers recently typed characters and pastes/expects only what
   the flood hasn't delivered yet (`already_typed_prefix_len`).

The swallow count is an estimate, hence the deadline: leaving a few
stray tail characters is a minor imperfection; eating the user's next
real keystrokes isn't.

## Pasting into / deleting from a search field

Reported: pasting into search fields was slow, and so was Backspace.
Measured first (release build, 100k-line file, temporary `#[ignore]`
benches, removed afterward):

| Where | Per keystroke, before | After |
|---|---|---|
| Editor `Ctrl+F` box: type a char | **2.46s** | ~0.5ms |
| Editor `Ctrl+F` box: Backspace | **0.59s** | ~15µs |
| Editor redraw (every keystroke) | ~30ms | ~1ms |
| Command line / Find file: key + redraw | 0.25ms | unchanged |

Three separate causes:

- **`edtui`'s search.** `edtui-jagged` 0.1.13's case-insensitive
  comparison (`jagged/match_indices.rs`) is
  `a.to_lowercase().collect::<String>() == b.to_lowercase().collect::<String>()`
  -- two allocations per character comparison, in a sliding window over
  the whole buffer, rerun from scratch on every query edit. `SearchState`
  is `pub(crate)`, so the whole mechanism was replaced
  (`SearchSession`, same semantics, checked against `edtui-jagged`'s own
  test fixture): no allocation per comparison, a longer pattern filters
  the previous candidates, Backspace pops a per-prefix stack.
- **Every redraw scanned the whole buffer** for other occurrences of
  the word under the cursor (1404 found, ~50 visible). Now limited to
  rows that can be on screen (`view.rs::rows_that_can_be_visible`).
- **Every other text field still got the ~7-8ms/char flood** -- their
  own handling was already fast. The physical-`Ctrl+V` bypass now
  covers an explicit allow-list of fields
  (`event_loop::paste::paste_target`). An allow-list, not "every mode
  but the editor": bracketed paste used to replay text into every
  non-editor mode, including ones where a letter is a command. Sharing
  that routing also fixed a paste going into the file's buffer instead
  of the open `Ctrl+F` box.

Found along the way, not fixed: an edit in the editor's own text costs
~29ms on the same file, almost all of it the undo snapshot's
full-buffer clone.
