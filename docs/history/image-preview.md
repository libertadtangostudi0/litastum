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
