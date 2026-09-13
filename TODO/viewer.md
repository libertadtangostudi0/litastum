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
- [x] **`.md`/`.markdown` preview** -- resolved the open design question
      in favor of an actually-rendered preview
      (`explorer/markdown_preview.rs`'s `render_markdown`, walking
      `pulldown-cmark`'s event stream), not just reusing the editor's
      own syntax-highlighted raw-text view: headings (bold, accent
      color), **bold**/*italic*, inline and fenced code (a distinct
      color), block quotes, ordered/nested/unordered lists (with real
      bullet/number prefixes and indentation), links (accent + underline,
      URL itself not shown -- a preview, not a browser), and horizontal
      rules. Same right-panel-replacement convention `F3`'s image
      preview already established (`Esc`/`F3` to close) -- `Up`/`Down`/
      `PageUp`/`PageDown` scroll instead of `Left`/`Right` cycling
      (there's no "next .md in the directory" concept requested the way
      there was for images). `pulldown-cmark` added with
      `default-features = false` (no `html`/`serde`/... -- only the
      core event-parsing API is needed, not HTML rendering). Tables,
      footnotes, task-list checkboxes, and images fall through
      un-rendered (a real gap, not a crash) -- not asked for, and this
      was never meant to be a full CommonMark-to-terminal renderer, just
      the constructs a real README/notes file actually uses.
- [x] **Clickable links** (mouse or touchpad) -- requested directly.
      `MarkdownSpan` keeps each link's own URL (`url: Option<String>`,
      never shown in the rendered text itself) so `Ctrl`+left-click can
      open it with the OS's default handler
      (`explorer::system_open::open` -- built for opening a file/
      directory in the OS file manager, but its per-OS commands
      (`explorer`/`open`/`xdg-open`) already hand a URL straight to the
      default browser too, no separate "open a URL" mechanism needed).
      `Ctrl` is required (not a plain click) -- requested directly after
      an initial plain-click version, matching the convention other
      terminals/editors use (VS Code's integrated terminal, iTerm2) so
      an ordinary click/drag is still free for the terminal's own use.
      Mouse capture (`crossterm`'s `EnableMouseCapture`) is turned on
      only for the lifetime of a markdown preview session
      (`open_preview`/`handle_markdown_preview_key`'s `Esc`/`F3` close
      path), not app-wide -- capturing the mouse takes over the
      terminal's own native text selection, which would otherwise get
      in the way of copying paths/command output with the mouse
      everywhere else in this file manager. The scroll wheel also
      scrolls the preview (`MouseEventKind::ScrollUp`/`ScrollDown`, no
      `Ctrl` needed) -- a natural, essentially free addition once mouse
      events were flowing through at all. `handle_markdown_preview_mouse`
      logs each click/hit-test result via `tracing::debug!` -- reported
      directly as "not working" once, with no other diagnostic
      available in this environment to narrow down which stage failed
      (event delivery, hit-testing, or the actual OS open call).

      **Another real bug, found right after**: clicking a link ended up
      opening a seemingly random folder (a user's OneDrive-redirected
      `Documents`) instead of the actual link. Root cause:
      `system_open::open` was called with the link's raw text totally
      unvalidated -- a real README commonly has in-document anchor
      links in its own table of contents (`[Installation](#installation)`)
      or bare relative references to other files in the repo
      (`[Contributing](CONTRIBUTING.md)`), neither of which
      `explorer.exe` can open directly; given garbage it doesn't
      recognize, it silently falls back to opening its own default
      location instead -- not a fixed, predictable folder, and nothing
      to do with the link that was actually clicked. Fixed with
      `resolve_link_target`: a bare `#fragment` now resolves to `None`
      (no-op -- jumping to an in-document heading isn't supported yet,
      there's no heading-to-line index for it), a real absolute URL
      (contains `://`, or `mailto:`) passes through to
      `system_open::open` as before, and anything else is treated as a
      relative reference to another file in the project -- resolved
      against the previewed file's own directory and opened *as a file*
      only if it actually exists on disk (a `#fragment` suffix on one of
      these, e.g. `readme.md#section`, is stripped before resolving).
      A relative reference to a file that doesn't exist also resolves to
      `None` -- same "never hand the OS a guess" rule as the anchor
      case.

      **Follow-up UX gap, reported right after**: once a click on an
      anchor/missing-file link correctly stopped opening the wrong
      thing, it went completely silent instead -- indistinguishable
      from the click not registering at all ("не знаю что происходит
      при кликах на ссылки"). Fixed with `MarkdownPreviewState::link_message`
      (`Option<String>`, set by `handle_markdown_preview_mouse` on
      *every* `Ctrl`+click attempt, success or not) shown as the
      preview panel's own bottom border title
      (`ui::markdown_preview::draw_markdown_preview`) -- "Opened: ...",
      "Failed to open ...: ...", "Can't open yet: ... (in-document
      anchor, or file not found)", or "no link on this line". Before
      the first click, that same spot shows a static "Ctrl+click a link
      to open it" hint instead, so the feature is discoverable without
      already knowing it exists.

      **Real crash, found immediately on retest**: `main.rs::restore_terminal`
      first sent `DisableMouseCapture` *unconditionally* on every exit,
      as a safety net for quitting (`F10`) while a preview happened to
      still be open -- but on Windows this crashed outright
      (`Error: 0: Initial console modes not set`) the moment `F10` was
      pressed in an ordinary session that never opened a Markdown
      preview at all: `crossterm`'s Windows console backend has no
      "initial mode" saved to restore unless `EnableMouseCapture` had
      actually run first in that process. Fixed with `App::mouse_capture_enabled`
      (set only once `EnableMouseCapture` truly succeeds, cleared once
      `DisableMouseCapture` truly succeeds) -- `restore_terminal` now
      only attempts `DisableMouseCapture` when that's `true`, so the
      safety net still catches "quit while previewing" without ever
      firing on a session that never touched mouse capture in the first
      place.

      **Also fixed, reported together**: a startup visual glitch (some
      stray text briefly visible in a panel before the real UI painted
      over it) traced to `App::image_picker`'s own startup query
      (`Picker::from_query_stdio`, `main.rs`) writing/reading raw escape
      sequences directly on stdio, bypassing `ratatui`'s own render
      buffer -- `ratatui`'s diffing render only rewrites cells that
      differ from its *own* last-known (initially blank) buffer, so a
      query artifact landing outside whatever the very first frame
      happens to redraw could otherwise linger. `terminal.clear()?`
      right after the query forces the next `draw()` to treat the whole
      screen as needing a full repaint, guaranteeing the first real
      frame overwrites everything.
- [x] **Keyboard-driven link search** (`l`, `Mode::MarkdownLinkSearch`)
      -- requested directly, after `Ctrl`+click's own row-based hit-
      testing (`link_at`) turned out to genuinely misfire in practice:
      it maps a screen row straight back to a logical-line index without
      accounting for `ratatui`'s own word-wrap, so it drifts by however
      many extra rows any *earlier* wrapped paragraph consumed -- common
      enough in a real document (confirmed against a real click that
      landed on the wrong line entirely) to make it unreliable as the
      *only* way to reach a link. `l` opens a filterable list of every
      link in the document (`MarkdownPreviewState::links()`, walking the
      same parsed `MarkdownLine`s `render_markdown` already produces --
      exact by construction, no screen-position guessing at all): typing
      narrows by label or URL (case-insensitive), `Up`/`Down` move,
      `Enter` opens the highlighted one and `Esc` cancels, both
      returning to the preview. Modeled on the editor's own
      `Ctrl+F` search box for the query field itself (append/backspace
      only, no cursor movement -- a short filter query has no real need
      for `text_field.rs`'s fuller editing machinery). `Ctrl`+click
      stays available too (`open_link`, the same resolve-and-open logic
      both paths now share) as a quick option for a short, unwrapped
      line -- not removed, just no longer the only option.

      **Follow-up: the line-level hit-testing gap above was reported as
      a real bug, not just accepted** -- a link that happened to sit
      right after a long, wrapped paragraph resolved to the wrong line
      when `Ctrl`+clicked, confirmed directly (fixing an unrelated
      anchor link to a full URL made the keyboard search find it, but
      `Ctrl`+click on that same link still failed). Asked explicitly to
      "fix the exact hit-test" rather than document the limitation or
      drop `Ctrl`+click entirely. Fixed by no longer letting `ratatui`'s
      own `Paragraph` wrap at all: `explorer::markdown_preview::
      wrap_markdown_line`/`wrap_ranges` reimplement its greedy
      word-wrap from scratch (word-boundary wrapping, hard-splitting an
      overlong single word), and `ui::markdown_preview::
      draw_markdown_preview` now wraps every visible logical line
      itself, renders the *already-wrapped* rows with no `Paragraph`
      `Wrap` at all, and builds `MarkdownPreviewState::visible_row_links`
      (`Vec<Vec<(col_start, col_end, url)>>`, one entry per rendered
      row) from those exact same rows. `link_at` now looks a click up
      directly in that table instead of reconstructing a logical-line
      index from `scroll() + row offset` -- rendering and hit-testing
      are provably derived from the identical wrapped rows, so they can
      no longer disagree. Covered by a unit-level `wrap_markdown_line`
      suite (word-boundary wraps, overlong-word hard-splits, a link
      spanning a wrap point) and one integration test that draws a real
      wrapped paragraph through `draw_markdown_preview` and confirms the
      link after it is still clickable.

      **Another real bug, found right after fixing the above**: a long
      opened URL truncated by the panel's own bottom-border width still
      *looked* like a complete, valid link once cut off mid-path (e.g.
      `.../blob/main/CONT`) -- reported directly as the link "looking
      shortened" and then 404ing when followed. Root cause: some
      terminals (Windows Terminal included) auto-detect and linkify
      URL-shaped plain text on their own, entirely outside this app's
      own `Ctrl`+click handling -- a user (or the terminal itself)
      following *that* auto-detected, truncated hyperlink lands on a
      real but broken address, which has nothing to do with this app's
      own hit-testing (already exact, per the fix above) or with
      `resolve_link_target`, which always resolves the link's real,
      untruncated `url`. Fixed by never putting a raw URL into
      `link_message` at all: `links::open_link` now names the link's
      own *label* (its rendered text, e.g. "before you start") instead,
      which is normally short, human-readable text that can't be
      mistaken for a real link even if the border does still truncate
      it. `MarkdownPreviewState::link_message`'s own field doc comment
      has the full mechanism.

      **Also reported alongside**: `Ctrl`+click on a link felt slow.
      `system_open::open` (`explorer.exe <url>`) hands a URL to
      Explorer's own already-running shell process over IPC -- correct
      for opening a folder window, but a real, perceptible round-trip
      just to launch a browser. Split into a distinct `system_open::open_url`
      for real URLs (`cmd /C start "" <url>` on Windows -- `cmd`'s
      builtin `start` calls `ShellExecute` directly, no Explorer IPC hop
      needed), matching the same `cmd /C start` idiom other
      cross-platform "open a URL" tools already use on Windows for this
      exact reason. `links::resolve_link_target` now returns a
      `LinkTarget` (`Url`/`File`) instead of always a `PathBuf`, so
      `open_link` can route each kind to the command that actually fits
      it -- a relative file reference still opens via `system_open::open`
      unchanged.

      **`explorer/markdown_preview.rs` was split into a directory**
      (`explorer/markdown_preview/{mod,render,wrap,state,links,input}.rs`
      + a sibling `tests.rs`) alongside the fixes above -- its
      production code alone (not counting tests) had already passed the
      project's own ~500-line decomposition threshold
      (`.claude/rules/code-conventions.md`), the same "mod.rs + tests.rs"
      pattern already used elsewhere in this codebase (`text_field/`,
      `editor/editor_keymap/`, ...), just also split by concern
      (rendering, wrapping, state, links, input) since the production
      code on its own needed more than a mechanical test-only move.
- [x] **`F3` on `.md`/`.markdown` now opens the built-in editor and the
      live preview side by side**, requested directly ("одновременно
      просматривать .md, редактировать его в левой панели и при
      сохранении смотреть что в правой (preview)") -- replaces the
      earlier read-only-only preview for these files entirely (the
      standalone `Mode::MarkdownPreview` variant, with no editor
      attached, is gone; `Mode::Editing`/`ConfirmDiscard` are reused
      instead, split into two panels instead of full-screen whenever
      `App::markdown_edit_preview` is `Some` -- see `Mode::Editing`'s
      own doc comment for why this reuses those two variants rather
      than adding parallel ones, which would have meant duplicating
      `editor_keymap.rs`'s Save/Close/discard-confirm logic). `Ctrl+S`
      still saves as `F4` always has, and now also calls
      `MarkdownPreviewState::reload()` to re-render the just-written
      file into the linked preview -- editing and checking the result
      is one continuous workflow instead of a separate preview-then-
      edit round trip. `App::active` (`0` = editor, `1` = preview)
      decides which half keyboard input reaches; `Tab`
      (`main.rs::handle_key_event`) toggles it -- deliberately claimed
      here the same way it's claimed for panel-switching in ordinary
      `Mode::Browsing`, so the embedded preview keeps its own
      `Up`/`Down`/`PageUp`/`PageDown` scrolling and `l`/`Ctrl`+click
      link interactivity (a real, explicit decision -- an earlier,
      simpler design considered making the embedded preview read-only/
      non-interactive, rejected once it became clear that would make
      `Ctrl`+click and `l`-search unreachable for `.md` files
      altogether, since there's no longer a preview-without-an-editor
      entry point). `l` on the preview half "parks" the `Editor` inside
      `Mode::MarkdownLinkSearch`'s own tuple (`Mode` can only hold one
      thing at a time) while its popup is up; `Esc`/`F3` on *either*
      half closes the whole session through the exact same
      `editor::close_editor_or_confirm` plain `F4` editing already
      uses, unsaved-changes prompt included, regardless of which side
      had focus when it was pressed.

      **Follow-up, requested directly** ("прокрутку текста надо сделать
      одновременной (схожий контент) примерно на одном уровне... выделить
      строку на превью, которая редактируется в редакторе"): the preview
      now scrolls to and highlights whichever rendered line corresponds
      to the editor's own cursor line, every frame the editor has focus
      (`App::active == 0`) -- `render_markdown` now also returns, per
      rendered line, the 0-indexed *source* line it started at
      (`Parser::into_offset_iter`'s own byte ranges translated to a line
      number), and `MarkdownPreviewState::sync_to_editor_cursor` maps
      `Editor::cursor_row()` to the closest matching rendered line
      (skipping blank separator lines, which share a source row with
      whatever block just closed and would otherwise be picked instead
      of the real content line right before them), and records it as
      `highlighted_line` for `draw_markdown_preview` to paint with
      `theme.current_row_bg` -- every wrapped visual row of a
      highlighted paragraph, not just its first. Approximate by
      construction (documented on `render_markdown` itself): a source
      paragraph spanning several lines collapses to one flowed preview
      line stamped with just its first source line, so mid-paragraph
      cursor positions all land on the same preview line -- matches
      what was actually asked for ("примерно", not byte-exact).
      Deliberately gated on `App::active == 0` so Tab-ing to the preview
      and scrolling it by hand isn't immediately undone the next frame
      just because the cursor hasn't moved.

      **Second follow-up, once the first (top-aligning) version was
      actually seen in use**: "можно их примерно на одном уровне держать
      по странице, если редактирование в середине страницы, то и превью
      в том же месте" -- always scrolling the matched line to the
      preview's own top kept the two in sync but put it at a visibly
      different *screen row* than the cursor whenever the cursor wasn't
      already at the very top of the editor's own visible page, which
      is what "held at the same level" actually meant. Fixed by matching
      the *relative* position instead of always top-aligning:
      `Editor::viewport_top_row` (`EditorState::viewport_offset`, the
      one thing `edtui` does expose about its own live scroll position)
      combined with `Editor::cursor_row` tells `ui::draw` how far down
      the editor's own visible page the cursor currently sits (`0.0` at
      the top, `1.0` at the bottom); `sync_to_editor_cursor` now takes
      that fraction plus the preview's own visible row count and offsets
      `scroll` back from the matched line by that same fraction of its
      own height, so editing halfway down the editor's page keeps the
      matching preview line roughly halfway down its own page too.
      Both panels' visible-row counts are derived the same way
      `draw_editor`/`draw_preview_frame` compute their own real content
      area (border/hint rows subtracted) rather than assumed.
- [ ] Widen image preview to `.gif`/`.webp` (the `image` crate can
      already decode both -- just a `Cargo.toml` feature-flag and
      `SUPPORTED_EXTENSIONS` change) -- not asked for explicitly, so not
      done yet.
- [ ] What does `F3` do on a file that's neither Markdown nor a
      supported image (e.g. a plain `.txt`, a binary, a directory)? Far
      Manager's own `F3` falls back to a plain text/hex view for
      everything -- not committed to matching that yet, just noting the
      question exists.
- [ ] Tables/task-list checkboxes/inline images in Markdown -- `explorer/
      markdown_preview.rs::render_markdown`'s own doc comment lists
      these as deliberately dropped for now; would need their own
      `MarkdownSpanKind`/layout handling (a table especially, since
      `ratatui`'s own `Table` widget doesn't compose with a flat
      `Vec<Line>` `Paragraph` the rest of this preview uses).
