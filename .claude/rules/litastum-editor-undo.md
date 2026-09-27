# litastum: built-in editor undo/redo

## `edtui::EditorState::capture()` is `pub(crate)` — unreachable from litastum

`capture()` is what records a checkpoint on `edtui`'s own undo stack
(`edtui-0.11.7/src/state/undo.rs` — a brute-force, whole-`Lines`-clone-
per-checkpoint stack, `max_size: 100`, oldest dropped once exceeded).
It's never exported past `pub(crate)`, so nothing outside the crate can
ever call it directly — a real, load-bearing constraint behind every
undo bug this editor has hit, not an oversight on our side to work
around differently next time.

`capture_on_insert: true` (`standard_key_handler()`'s own config, see
[[litastum-stack]]) makes `edtui`'s own `KeyEventHandler::on_event`
call `capture()` internally, but only ever for `KeyCode::Char`/`Tab`
while in `Insert` mode — never for `LineBreak` (Enter), and never for
anything our own fast paste path does, since that path bypasses
`edtui`'s dispatch entirely for speed (`editor/editor/fast_paste.rs`,
see [[litastum-performance]] for why the paste itself needs to be
fast). So a fast paste could never register on `edtui`'s real stack in
the first place — the boundary between "before the paste" and "the
pasted content" simply didn't exist there.

## Why a single dedicated snapshot wasn't enough

First fix: `Editor` kept one extra, hand-rolled snapshot
(`paste_undo`/`PasteUndo`) purely for the most recent paste, cleared on
any other key. This covers exactly one edit after the paste, no more —
reported broken directly once a real sequence went paste → type a
character or press Enter → several `Ctrl+Z` presses: the dedicated
snapshot was already cleared by the first non-paste key, so further
undos fell through to `edtui`'s real `Undo`, which had no checkpoint at
all for "before the paste" (see above) but *did* have one per typed
character (`capture_on_insert`). Result: the typed characters undid
correctly, one at a time, but once undo reached back into the pasted
text itself, there was nothing left but character-level granularity to
fall back to — reported directly as "если вставка была куском, отмена
тоже должна применяться к куску", i.e. undo deleting the paste one
character at a time instead of as the one block it was inserted as.

A second attempt (a `Vec<PasteUndo>` stack instead of one slot) only
fixed a narrower, different complaint — consecutive pastes each only
getting one level of undo — and didn't touch this root cause at all,
since it was still a paste-only mechanism with the same "cleared by any
other key" lifetime.

## The actual fix: `Editor` owns a complete undo/redo stack for `Standard` keymap

`Editor::undo_stack`/`redo_stack: Vec<Snapshot>`
(`editor/editor/mod.rs`) fully replace reliance on `edtui`'s own
`Undo`/`Redo` actions and `capture_on_insert` for the `Standard`
keymap — not just for paste, for every mutating key:

- `Editor::input` intercepts `Ctrl+Z`/`Ctrl+Y` directly, ahead of
  `edtui`'s own dispatch, calling this app's own `undo`/`redo` instead
  of ever reaching `edtui`'s real `Undo`/`Redo` actions for this
  keymap.
- A snapshot is pushed before every key that could plausibly mutate the
  buffer (`should_capture_undo_snapshot`, built on the pre-existing
  `can_mutate_buffer` predicate, with one added exclusion: `Ctrl+C`/
  copy, which never mutates but isn't in `can_mutate_buffer`'s own
  navigation-key list) — so Enter, typing, and `paste_text` all now
  push exactly one boundary each, on equal footing, unlike
  `capture_on_insert`'s Char/Tab-only, Insert-mode-only reach.
- A real edit's own snapshot is never discarded by an unrelated later
  key the way the old paste-only mechanism discarded itself — pure
  navigation (arrows, Home/End, ...) leaves the stack untouched
  entirely, and each further real edit pushes its own new entry on top
  rather than clearing what came before it. This is what makes a
  paste's own boundary survive arbitrary later edits: it's just one
  more entry on one continuous stack, never a special case with its
  own shorter lifetime.

**Deliberately not comparing buffer state before/after a keystroke to
detect true no-ops** before pushing a snapshot — this project has
explicit prior history (`Editor::dirty`'s own doc comment) of moving
*away* from O(buffer-length) comparisons on every keystroke, after a
pathologically long single line made every arrow-key press laggy. The
one known false positive this could cause (`Ctrl+C`/copy pushing a
snapshot even though it never mutates the buffer) is handled instead by
an explicit, cheap exclusion in `should_capture_undo_snapshot` — not a
reason to reintroduce a full-buffer diff on every key.

`Vim` keymap is completely unaffected by any of this — it keeps using
`edtui`'s own native `capture_on_insert`/`Undo`/`Redo` mechanism
exactly as before, matching this project's established "no correction
pass runs for Vim" rule (see [[litastum-stack]]).

## Memory: `Limits::max_paste_undo_stack`

Each `Snapshot` is a full clone of the entire buffer (`Lines` +
cursor) — the same "brute-force, whole-state" shape `edtui`'s own
internal undo stack already uses. Given this project's own stated
target scale (tens, sometimes hundreds of thousands of lines, see
[[litastum-performance]]), an unbounded stack could pile up an
unbounded number of full-buffer clones. `Limits::max_paste_undo_stack`
(default 20, `config.json`-overridable — see [[litastum-config]]) caps
it, oldest entry dropped first, matching `max_command_history`'s own
"cap and drop the oldest" convention.

## Regression coverage

`editor/editor/tests.rs::undo_treats_a_paste_as_one_block_even_after_later_edits`
is the direct regression test for the reported bug: paste → type two
characters → three `Ctrl+Z` presses → the two typed characters undo
individually first, then the entire pasted block disappears in the one
further `Ctrl+Z`, not character by character.
