# Image preview (F3 on an image) -- history

Code: `src/explorer/image_preview/mod.rs`, `src/ui/image_preview.rs`.
Crate choice and the terminal-protocol query: `litastum-stack.md`.

- **Browsing, not a one-shot preview.** Requested directly: `Left`/
  `Right` cycle through the directory's images. `F3` also makes the
  right panel active, since that's the panel whose area shows the
  preview and where the arrows then make sense.
- **Forced half-blocks looked unacceptably blocky.** A first version
  used `Picker::halfblocks()` unconditionally, assuming (untested) that
  capability probing would be unreliable on Windows. `App::image_picker`
  now comes from `Picker::from_query_stdio()` at startup, which never
  blocks it: real terminals answer at once, ConPTY falls back after its
  own 2-second timeout.
- **Decoding froze the UI.** `image` decode plus the protocol's
  resize/re-encode (Lanczos3, chosen for quality) took visible time
  for anything but a tiny image, both on the first `F3` and on every
  `Left`/`Right` -- all inline in the single-threaded key handler. Now
  on a background thread (`spawn_decode`); `poll` picks up the result
  from `event_loop::wait_for_event` and once more from `ui::draw`.
  A corrupt first image shows `Failed` instead of `open` refusing to
  enter the preview, as the synchronous version did.
- **A `Loading` placeholder on every switch flashed**, even for images
  that decode fast enough to look instant. Reverted: `step` keeps the
  previous image's pixels until the new one is ready; only the title
  (`current_path`) moves immediately. A failed decode likewise keeps a
  working previous image on screen.
- **No stale-result tracking needed**: only one `PendingDecode` lives
  at a time, and replacing it drops the old `Receiver`; the old
  thread's `send` then fails silently.
- **Sort order** is case-insensitive by name, not `Panel`'s natural
  sort (`pub(super)` to `panel`) -- "10.jpg before 2.jpg" is an accepted
  gap.
- **Only jpg/jpeg/png/bmp**, the formats requested (`TODO/viewer.md`).
  More need widening `Cargo.toml`'s `image` features too, kept narrow on
  purpose.
- **The resize filter.** `Resize::Fit` defaults to `Nearest`, reported
  as looking terrible on a screenshot with small text (one source pixel
  per cell, the rest thrown away). `Lanczos3` blends properly; it's
  slower, but runs once per image, not per frame.
- **Stray text before the first frame.** The protocol query writes to
  the real screen behind `ratatui`'s buffer, and some text showed in a
  panel until overwritten. `main` clears the terminal after the query.

## Switching images was very slow in Windows Terminal

Reported. Measured on 1100x684 screenshots in a debug build: decoding
38 ms, but resizing (`Lanczos3`) and encoding to Sixel 3.2 s -- done by
`StatefulImage` while drawing, on the UI thread, which froze meanwhile
(iTerm2 1.8 s, half-blocks 1.5 s). Two causes, two fixes:
- **The image crates ran unoptimized in debug builds.** Sixel's colour
  quantizer (`quantette`, `palette`, `wide`, ...) above all. They get
  `opt-level = 3` in `[profile.dev.package]` like the editor's crates:
  Sixel 3.2 s -> 67 ms, iTerm2 -> 50 ms, decoding -> 2 ms.
- **Resizing and encoding moved off the UI thread.** A job decodes,
  resizes and encodes for the preview's size (`Picker::new_protocol`,
  drawn with the plain `Image` widget); the size comes from the first
  draw (`set_area`, shared with the jobs; waited for up to 200 ms), and
  a new size encodes again from the decoded pixels. The two neighbors
  are prepared once the current image is ready, so `Left`/`Right` show
  at once; only those three are kept.

