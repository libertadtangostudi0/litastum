# litastum: performance target for the built-in editor

History (the three-round paste investigation, the flood-swallow bugs,
the search-field measurements): `docs/history/editor-performance.md`.

## Scale this app is expected to handle

The built-in editor (F4) needs to stay usable on real files in the
**tens of thousands of lines**, sometimes **hundreds of thousands** --
not just the small fixtures most tests open. Every perf fix so far was
written against a real, reported symptom on a real file. Consider any
new editor feature (a correction pass in `editor/bindings`, a per-frame
highlight scan, ...) against this scale before landing it, the way
`word_highlight.rs::has_pathologically_long_line`/
`MAX_HIGHLIGHTED_LINE_LEN` gate highlighting for one very long line,
and word-occurrence highlighting only scans rows that can be on screen
(`view.rs::rows_that_can_be_visible`).

## Paste: how it works now

- **The insert**: `Editor::paste_text` (`editor/editor/fast_paste.rs::splice_paste`)
  splices the text into the row with `Vec::splice` -- never `edtui`'s
  own paste, which inserts one character at a time (O(n^2) on a long
  line). One undo snapshot per paste ([[litastum-editor-undo]]).
- **Windows Terminal owns `Ctrl+V`**: it consumes the key and injects
  the clipboard as simulated keystrokes at ~7-8ms each. So the physical
  key is polled with `GetAsyncKeyState` on every loop iteration
  (`windows_terminal/paste_hotkey.rs`), the clipboard is read and pasted
  at once, and the flood that follows is swallowed
  (`windows_terminal::PasteFlood` -- matched character by character,
  with a deadline as a safety valve; see its own docs).
- **Where a paste lands**: `event_loop::paste::paste_target`, an
  explicit allow-list -- the editor buffer, or a single-line text field
  (replayed with no redraw in between). A mode where a letter is a
  command never gets pasted text.
- **Bracketed paste** (`EnableBracketedPaste`, `Event::Paste`) goes
  through the same routing. It only works on Unix: `crossterm`'s
  Windows backend never produces `Event::Paste`.
- **A burst of already-queued typed characters** is applied in one
  `paste_text` call with one redraw
  (`event_loop::keys::drain_pending_editor_typing`).

## Search

The editor's `Ctrl+F` search is our own `SearchSession`
(`editor/editor/search/session.rs`), not `edtui`'s: `edtui-jagged`'s
case-insensitive comparison allocates per character and reran over the
whole buffer on every query edit (2.46s per typed character on 100k
lines). Ours is allocation-free and incremental.

## Editing: nothing O(buffer) per keystroke

Measured on a 300k-line, 53 MB log, typing a character cost 246 ms in
a release build (367 ms in debug) -- three whole-buffer passes per key.
Now ~2 ms:

- **Undo keeps one row for one-row edits** (`undo::Change::Row`):
  typing, Tab, Backspace/Delete within a row, a one-line paste. Enter,
  multi-line paste and deleting a selection still copy the buffer.
- **`edtui`'s own history is off** (`standard_key_handler(false)`): it
  copied the buffer before every typed character, unused, unbounded.
- **`is_dirty` is tracked per edit** (`changes::Differing`) against one
  hash per saved row (`changes::SavedRows`), hashed lazily before the
  first edit -- no second copy of the buffer in memory.
- **The long-line check is cached** (`Editor::has_long_line`), not a
  scan per frame.

A debug build also optimizes the editor's heavy crates (`edtui`,
`ratatui`, `syntect`, `similar`, ...; `[profile.dev.package]` in the
root `Cargo.toml`): a frame went from ~40 ms to ~4 ms. litastum's own
code stays unoptimized there, so the first edit after opening such a
file pauses ~0.9 s to hash its rows (~0.1 s in release).

Compare and the conflict resolver diff only when a text changes
(`compare::DiffCache`, keyed on `Editor::revision`), and an editor hands
`edtui` only the visible rows' extra highlights (`highlights_on`): it
checks every highlight it gets per visible line.

Still O(buffer): opening (~0.3 s release), the diff after an edit in
Compare, and the first character typed into the `Ctrl+F` box (~150 ms
release on that log).

## The general lesson

The paste report took three rounds, each a plausible, source-backed
theory that turned out wrong (or incomplete) for the reported symptom
-- only re-measuring against the real scenario (a direct benchmark, the
log's own timestamps) found the actual cause. A future "the editor
feels slow" report deserves the same: measure the real scenario first,
don't assume last time's cause.
