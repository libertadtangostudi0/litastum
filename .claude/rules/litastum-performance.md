# litastum: performance target for the built-in editor

## Scale this app is expected to handle

The built-in editor (F4) needs to stay usable on real files in the
**tens of thousands of lines**, sometimes **hundreds of thousands** --
not just the small fixtures most of this codebase's own tests open.
This isn't a hypothetical future concern: every perf fix below was
written against a real, reported symptom on a real file, not a
synthetic benchmark invented ahead of time. Any new editor feature
(a correction pass in `editor/bindings`, a per-frame highlight scan,
...) should be considered against this scale before landing, the same
way `word_highlight.rs::has_pathologically_long_line`/
`MAX_HIGHLIGHTED_LINE_LEN` already gate syntax highlighting and
word-occurrence scanning for a single pathologically long *line*.

## Case study: pasting felt "like watching it render line by line"

Reported directly, with that exact phrasing — pasting a real ~20KB/
338-line file into the editor took well over a minute, later
confirmed to sometimes run into multiple unresponsive minutes on a
larger paste. Tracing this took three separate rounds, each one
disproving the previous theory instead of confirming it — worth
reading in full before assuming a future editor-performance report has
the same cause as a past one.

**Round 1 — assumed it was `edtui`'s own paste algorithm.** Confirmed
directly from source (`edtui` 0.11.7's `actions/cpaste.rs::paste` ->
`helper::insert_str` -> `insert_char`): pasted text is inserted *one
character at a time*, each call doing its own `Vec::insert` at a
column that keeps growing — an O(current line length) shift, every
single character. `edtui-jagged`'s own `Jagged` type has no bulk
"splice N elements into a row" API at all to have used instead (only a
single-element insert or a whole-new-row insert). This is a real,
confirmed O(n²) cost for a long single-line paste — genuinely worth
fixing, and fixed (`editor/editor/fast_paste.rs::splice_paste`, a
from-scratch `Vec::splice`-based insert, O(pasted length + line
length)), but turned out **not to be what the actual report was about**
once measured: a benchmark against the real reported file showed
`splice_paste` itself completing in under 1ms, nowhere close to
"minutes." Kept anyway — it's a real fix for a real, if different,
pathological case (a single enormous line with no newlines, e.g. a
JSON blob or a giant escaped log line) — but the search for the actual
reported bug had to continue.

**Also lost in this fix**: `edtui`'s own undo integration —
`EditorState::capture()` (what records an undo checkpoint) is
`pub(crate)`, unreachable once the buffer is spliced directly instead
of going through a real `Execute`-able action. Since "`Ctrl+Z` undoes
exactly the paste, and both have to be fast" was itself an explicit
requirement, `Editor` first kept its own one-shot paste snapshot, and
later took over the whole undo/redo stack for the `Standard` keymap
(`editor/editor/undo.rs`) — see [[litastum-editor-undo]] for why the
one-shot version wasn't enough.

**Round 2 — assumed it was one redraw per pasted character.** This
app's own main loop (`event_loop::run`) redraws once per handled
`crossterm` event, and pasting without a terminal's "bracketed paste"
feature enabled means every pasted character arrives as its own
separate `Event::Key`, indistinguishable from a real keystroke — so a
20,000-character paste genuinely was 20,000 redraws. Enabled
`crossterm::event::EnableBracketedPaste` (`setup_terminal`) and handled
the resulting `Event::Paste(String)` as one atomic block
(`event_loop::paste::handle_paste_event`) — correct, and a real improvement in
principle, but **also not what fixed the actual report**: `bracketed-paste`
is a real Cargo feature crossterm ships (on by default), but confirmed
directly from `crossterm` 0.29's own source that its *Windows* backend
(`event/sys/windows/`) has no code path that ever produces
`Event::Paste` at all — bracketed paste there is Unix-only, built on
parsing raw ANSI escape bytes out of stdin, which the Windows Console
API backend (structured `KEY_EVENT_RECORD`s via `ReadConsoleInputW`,
never a raw byte stream) simply never does. `EnableBracketedPaste` was
dead code for this user's actual platform. Kept anyway (harmless,
correct on Unix, and `event_loop::keys::drain_pending_editor_typing`'s
"coalesce a burst of already-queued plain characters into one
`Editor::paste_text` call, redraw once" fix built alongside it is a
real, general improvement for any burst of queued key events, pasted
or just fast typing) — but still hadn't found the real bottleneck.

**Round 3 — the actual cause, found from `logs/litastum.log`'s own
timestamps.** Every consecutive pasted character's own `editor key`
debug line was **~7-8ms apart, like a metronome** — not a redraw cost
(already ruled out) and not `splice_paste`'s own cost (already ruled
out), but the literal *arrival rate* of the characters themselves. Root
cause: Windows Terminal binds `Ctrl+V` to its own "paste" action and
consumes the keystroke *before* it ever reaches this app — confirmed by
the same log never showing a real `Ctrl+V` `KeyEvent` at all during a
paste, only the flood of plain characters that follows. Windows
Terminal then injects the clipboard text into the console's input
stream as simulated keystrokes at that same throttled rate, because
(per Round 2) there's no bracketed-paste negotiation happening that it
could use instead. Neither this app's own redraw cost nor its own
insert algorithm could ever fix this — the bottleneck is *upstream*,
in how fast the terminal chooses to feed characters in, over which this
app's normal `crossterm` event handling has no influence at all.

**The actual fix**: stop waiting for `crossterm` to ever report a real
`Ctrl+V` `KeyEvent`, and poll the *physical* key state directly via the
Windows API instead — `GetAsyncKeyState`, the exact same technique
`alt_key.rs` already uses for tracking real `Alt` hold/release (see its
own module doc for why `crossterm` can't report a bare modifier key on
Windows either). `paste_hotkey.rs` + `event_loop::paste::try_intercept_paste_hotkey`:
the instant a real physical `Ctrl+V` is detected, read the OS clipboard
directly and apply `Editor::paste_text` immediately — full speed,
independent of whatever Windows Terminal does next. Windows Terminal's
own (now redundant, still-slow) keystroke flood keeps arriving right
afterward regardless — there's no way to tell it to stop — so
`App::pending_paste_swallow`/`_deadline` silently discard that
predictable tail (`event_loop::paste::should_swallow_paste_tail`) instead of
typing the same text a second time. The swallow count is an *estimate*
(pasted-text length, `\r`-stripped), not a guarantee the flood matches
exactly, which is why the deadline safety valve exists: undercounting
and leaving a few stray tail characters is an acceptable, minor
imperfection; overcounting and silently eating the user's own *next*
real keystrokes after the paste would not be.

## Case study: pasting into / deleting from a search field

Reported directly: pasting into search fields was slow, and so was
deleting with Backspace. Measured first, per the lesson below (release
build, a real-scale file of 100k lines, temporary `#[ignore]` benches,
removed afterward):

| Where | Per keystroke, before | After |
|---|---|---|
| Editor `Ctrl+F` box: type a char | **2.46s** | ~0.5ms |
| Editor `Ctrl+F` box: Backspace | **0.59s** | ~15µs |
| Editor redraw (every keystroke) | ~30ms | ~1ms |
| Command line / Find file: key + redraw | 0.25ms | unchanged |

Three separate causes, each fixed where it actually lived:

- **`edtui`'s own search was the whole cost of the `Ctrl+F` box**:
  `edtui-jagged`'s case-insensitive char comparison allocates two
  `String`s per comparison, inside a sliding window over the whole
  buffer, rerun from scratch on every query edit. Its `SearchState` is
  `pub(crate)`, so the mechanism was replaced wholesale
  (`editor/editor/search/session.rs::SearchSession` -- same semantics,
  checked against `edtui-jagged`'s own test fixture): no allocation per
  comparison, and incremental -- a longer pattern only filters the
  previous candidates, and Backspace pops a per-prefix stack.
- **Every editor redraw scanned the whole buffer** for other
  occurrences of the word under the cursor (1404 found, ~50 visible).
  Now limited to rows that can possibly be on screen
  (`view.rs::rows_that_can_be_visible`).
- **Pasting into any field other than the editor's own buffer** still
  arrived as Windows Terminal's ~7-8ms/char keystroke flood (the Round 3
  cause above) -- the fields' own handling was already fast. The
  physical-`Ctrl+V` bypass now covers an explicit allow-list of text
  fields (`event_loop::paste::paste_target`), not just the editor.

Not fixed here, found along the way: Backspace (or any edit) in the
editor's own *text* costs ~29ms on the same file, almost all of it the
undo snapshot's full-buffer clone ([[litastum-editor-undo]]).

## The general lesson

Three plausible-sounding theories in a row, each backed by real source
reading and real reasoning, each wrong (or at best incomplete) about
what the *actual* reported symptom's cause was — confirmed by *not*
skipping straight to "fix and ship," but actually re-measuring against
the real reported file/scenario each time (a direct benchmark for round
1, the log's own timestamps for round 3) before believing a fix had
landed. All three fixes were worth keeping regardless (each is a real,
separate improvement, and none of them regressed anything), but only
the third one was ever the actual reported bug. A future "the editor
feels slow" report deserves the same treatment — re-measure against the
real scenario, don't assume it's the same cause as last time just
because the symptom sounds similar.
