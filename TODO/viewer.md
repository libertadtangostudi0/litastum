# File viewer (F3)

Requested directly. `F3` is already shown in the F-key bar as `View`
(`ui/mod.rs`'s `DEFAULT_LABELS`/`ACTIVE_PANEL_LABELS`) -- image preview
below is the first thing actually wired up to it. Read-only, distinct
from `F4`'s built-in *editor* -- same Far Manager convention this whole
app is otherwise following (`F3` view vs. `F4` edit).

- [x] **Image preview** (`.jpg`/`.jpeg`/`.png`/`.bmp`) --
      `explorer/image_preview.rs` (`ImagePreviewState`, `open_preview`,
      `handle_image_preview_key`) + `ui/image_preview.rs`
      (`draw_image_preview`). `F3` on a supported image switches the
      *right* panel active (`app.active = 1`) and replaces its own file
      listing entirely with the decoded image (`ratatui-image`'s
      `StatefulImage`/`StatefulProtocol`). `Left`/`Right` cycle to the
      previous/next supported image in the same directory (case-
      insensitive filename order, wrapping at both ends) -- requested
      directly ("просмотр изображений клавишами влево вправо внутри
      директории, выбрав правую активную панель"), not just a one-shot
      single-file preview. `Esc`/`F3` again closes back to
      `Mode::Browsing`. `image`/`ratatui-image` both trimmed to
      `default-features = false` with just the `jpeg`/`png`/`bmp`
      decode features and the `crossterm` picker backend -- the
      crates' own full default feature sets pull in heavy AV1/WebP
      encoders (`rav1e`/`ravif`) this app never needs. Confirmed:
      "react differently per format" needs no hand-written dispatch at
      all -- `image::ImageReader::open(path).decode()` already picks
      the right decoder from the file's own extension/signature on its
      own.

      **Rendering quality went through two rounds**, both reported
      directly against real screenshots: a first version forced
      `Picker::halfblocks()` unconditionally (reasoned, without
      testing, that real-terminal capability probing would be
      unreliable on Windows) -- looked "terrible," blocky nearest-
      neighbor artifacts on any fine detail (small text). First fix
      (`ui/image_preview.rs`) switched `Resize::Fit`'s default filter
      from `FilterType::Nearest` to `FilterType::Lanczos3` -- better,
      but still fundamentally limited by half-blocks' own low
      resolution (2 "pixels" per terminal cell), reported as still not
      good enough. Second, real fix: `App::image_picker` is now
      queried once against the *real* terminal at startup
      (`Picker::from_query_stdio()`, `main.rs`, called right after
      entering the alternate screen but before the main loop reads any
      keyboard events -- required ordering, the query itself writes/
      reads raw escape sequences on stdio) instead of assuming
      half-blocks -- a capable terminal (Sixel/Kitty/iTerm2 -- Windows
      Terminal now included) renders close to a real image; only a
      terminal that doesn't answer (legacy Windows Console/ConPTY)
      falls back to half-blocks, which `ratatui-image` itself already
      handles safely (2-second timeout, never blocks startup).
- [ ] **`.md` preview, next** -- the starting text format. Open question,
      not yet decided: a plain read-only view of the file (reusing the
      built-in editor's own Markdown syntax highlighting, already
      wired up per `.claude/rules/litastum-theming.md`'s "Syntax
      highlighting" section -- `!theme.rs`'s `markup.*` scopes), or an
      actually-rendered preview (headings/bold/lists/links shown
      formatted, not as highlighted raw text, closer to a browser's
      Markdown rendering)? These are two different features with very
      different scope -- the first is close to free (open the file in
      `Editor`, just force read-only/no-edit-keys), the second needs a
      real Markdown-to-`ratatui`-widgets renderer (no such crate
      dependency exists in this project yet). Don't start
      implementation from this bullet alone until this is settled.
- [ ] Widen image preview to `.gif`/`.webp` (the `image` crate can
      already decode both -- just a `Cargo.toml` feature-flag and
      `SUPPORTED_EXTENSIONS` change) -- not asked for explicitly, so not
      done yet.
- [ ] **Is `F3` one mode that branches by file extension, or two
      separate concerns (`TextView`/`ImageView`)?** Not decided --
      whichever shape makes `main.rs`'s own dispatch (`Mode` enum,
      `handle_event`'s match) and `ui/mod.rs`'s render dispatch cleanest
      once both previews actually exist; don't guess this ahead of
      having both to compare.
- [ ] What does `F3` do on a file that's neither Markdown nor a
      supported image (e.g. a plain `.txt`, a binary, a directory)? Far
      Manager's own `F3` falls back to a plain text/hex view for
      everything -- not committed to matching that yet, just noting the
      question exists.
