# Event loop and redraws -- history

Code: `src/event_loop/mod.rs` (`run`, `wait_for_event`,
`drain_pending_mouse_events`, `sync_mouse_capture`),
`src/event_loop/keys.rs` (`handle_key_event`, key-repeat draining),
`src/ui/mod.rs::draw`. Paste-specific history is in
`editor-performance.md`.

## The terminal cursor flickered to the end of the line

`ratatui`'s `Terminal::draw` applies `Frame::set_cursor_position` after
the buffer diff as `show_cursor()` then `set_cursor_position()`, and
`ratatui-crossterm` flushes each of those with `execute!` on its own
(cell writes are `queue!`d). So the cursor became visible wherever the
diff's last `MoveTo` left it -- for `Delete` on the command line, the
end of the line -- and jumped to its real spot on the next flush (both
confirmed from source). `ui::draw` now *returns* the cursor position
instead of setting it; `event_loop` applies it after the frame is on
screen, position first, then `show_cursor`.

A related flicker: Windows reports a `Release` event for every key, and
the loop used to redraw for it too -- the cursor briefly visited
wherever that frame left it (`Delete`/`Backspace` on the command line).
A key nothing was dispatched for no longer redraws.

## Panels crammed into one column

- **At startup**: `Panel::new()` starts with one column and
  `visible_rows = 0`, and the real values from `ui::draw` only reach the
  panels *after* the frame drawn with the placeholders. `wait_for_event`
  then blocks, so the one-column frame stayed until the next key. `run`
  now draws once up front to seed real values.
- **Every frame of a full-screen mode**: while the editor or Compare
  took the screen, `ui::draw` returned a placeholder layout, which `run`
  applied to both panels every frame -- closing the editor showed the
  crammed layout for a frame. It now returns each panel's own current
  values instead, making the apply a no-op.

## Bursts of events

- **Touchpad scrolling lagged** behind the gesture: one gesture is a
  burst of small wheel ticks, and the loop redrew after each.
  `drain_pending_mouse_events` handles the whole queued burst and draws
  once. A 16ms cap was added so a direction reversal couldn't hide
  behind a long backlog, but it made long scrolls redraw ~60 times a
  second and feel slower; removed, since stopping the drain at an actual
  direction change (`last_scroll`) already covers reversals.
- **Held `Up`/`Down`/`PageUp`/`PageDown` overshot** -- the editor caret
  kept moving after the key was released. Windows reports key repeats
  as ordinary presses, so each got a full dispatch and redraw. The same
  drain now coalesces same-axis repeats and stops on a reversal. Only
  these four keys: `Left`/`Right` backlogs are too short to notice, and
  coalescing typing would be wrong.

## Background work

The image preview's decode and the Find file search both moved off the
main thread after blocking the UI; `wait_for_event` polls them on a
short interval so a finished result shows without waiting for input.

## Mouse capture only in the editor

Capture takes over the terminal's own text selection, which the panels
and command line need for copying. It's on exactly while the editor is
open (for clicking the caret around, VS Code-style), synced once per
loop iteration rather than toggled at each of the several places an
editor opens or closes. `Shift`+drag still selects natively in Windows
Terminal. `DisableMouseCapture` without a prior enable crashed on
Windows ("Initial console modes not set"), hence
`App::mouse_capture_enabled`.
