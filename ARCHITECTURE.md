# litastum — architecture

Originally a pre-implementation sketch derived from the planning chat +
a Far Manager UI mockup; rewritten here to describe the actual current
module structure instead of the plan, now that most of the sketch has
long since landed (and, in a few places, differently than first drawn).
See `.claude/rules/*.md` for the design decisions and conventions
behind each area in more depth — this file stays at the module-map /
data-flow level.

## Module map

```
main.rs              — terminal setup/teardown, event loop, dispatches
                        by Mode to each area's own handle_*_key
app.rs                — App: panels, active index, Mode, theme,
                        popup_style, command/search history, ...
test_support.rs       — shared test fixtures (test_app, key(), scratch dirs)
alt_key.rs            — Windows-only: polls GetAsyncKeyState for real
                        Alt hold/release tracking (ui::draw_function_keys)
logging.rs            — tracing setup

explorer.rs            — dual-pane browser: Panel, F-key commands,
  explorer/keymap.rs    —   KeyCode -> Command
  explorer/command.rs   —   Command enum + execute(); the one chokepoint
                            keyboard (and, later, scripts) funnels
                            filesystem actions through
  explorer/panel/       —   Panel: cwd, entries, cursor, column-major
                            nav, scrolling, marks (marks.rs), natural
                            sort (natural_sort.rs)
  explorer/entry.rs     —   Entry, HighlightRole (file-type coloring)
  explorer/fs_ops.rs    —   actual copy/move/delete filesystem calls
  explorer/confirm.rs   —   Mode::ConfirmDelete/ConfirmTransfer key handling
  explorer/drive_menu.rs—   Alt+F1/F2 "change drive" popup state
  explorer/find_file/   —   F9 -> Commands -> Find file (search, state,
                            input handling, .txt export)
  explorer/system_open.rs — Shift+Enter: hands a path to the OS's own
                            file manager (explorer.exe/open/xdg-open)
  explorer/image_preview.rs — F3 on a supported image (jpg/jpeg/png/bmp):
                            Mode::ImagePreview, ImagePreviewState (decode
                            + Left/Right cycling within the directory via
                            ratatui-image); ui/image_preview.rs draws it
                            into the right panel's own area in place of
                            its usual file listing. See TODO/viewer.md.
  explorer/markdown_preview/ — F3 on a .md/.markdown file: opens the
                            built-in editor (left panel, Mode::Editing --
                            reused as-is, not a separate Mode, so it
                            keeps editor_keymap.rs's Save/Close/discard-
                            confirm logic unchanged) plus a linked live
                            preview (App::markdown_edit_preview, right
                            panel), refreshed on every Ctrl+S. App::active
                            (0/1) picks which side keyboard input
                            reaches; Tab toggles it (main.rs). Split into
                            a directory once the single-file version
                            passed ~1500 lines (production code alone
                            was already over the ~500-line threshold,
                            not just its tests -- see
                            .claude/rules/code-conventions.md):
    mod.rs                  —   MarkdownSpan/MarkdownSpanKind/
                            MarkdownLine (domain-only, no ratatui
                            dependency here -- same split HighlightRole
                            has from its own color mapping),
                            is_markdown_file, PAGE_SIZE, re-exports
    render.rs                —   render_markdown: pulldown-cmark event
                            stream -> styled MarkdownLine/MarkdownSpan
    wrap.rs                   —   wrap_markdown_line/wrap_ranges
                            reimplement ratatui's own greedy word-wrap
                            so draw_markdown_preview can render
                            already-wrapped rows (no Paragraph Wrap) and
                            build MarkdownPreviewState::visible_row_links
                            (exact per-row column hitboxes) from the
                            identical rows -- link_at looks a click up
                            in that table directly, so rendering and
                            hit-testing can't disagree
    state.rs                  —   MarkdownPreviewState: content_area,
                            link_message (always built from a link's
                            label, never its raw URL -- see its own doc
                            comment for the truncated-URL-turns-into-a
                            -broken-but-real-looking-link bug this
                            avoids), visible_row_links, scroll, link_at,
                            reload() (re-renders from disk after a save)
    links.rs                   —   MarkdownLink, MarkdownLinkSearchState,
                            LinkTarget (Url/File), resolve_link_target,
                            open_link -- dispatches a resolved Url to
                            system_open::open_url (cmd /C start, no
                            Explorer IPC hop -- reported directly as
                            feeling slow through system_open::open's
                            explorer.exe path) and a resolved File to
                            system_open::open
    input.rs                    —   open_edit_preview (Editor::open +
                            MarkdownPreviewState::open, links them via
                            App::markdown_edit_preview),
                            handle_markdown_edit_preview_key (App::active
                            == 1: scroll, l, Esc/F3 -> editor::close_editor_or_confirm),
                            open_link_search, handle_markdown_link_search_key,
                            handle_markdown_preview_mouse
    (tests split the same way, one sibling tests.rs)
    ui/markdown_preview.rs maps MarkdownSpanKind to a real Style and
                            draws into the right panel's own area, same
                            replacement convention as image_preview.rs.
                            Links: Ctrl+click (exact hit-test) or `l` ->
                            Mode::MarkdownLinkSearch (a filterable list
                            from MarkdownPreviewState::links(), parking
                            the Editor in its own tuple meanwhile) -- both
                            resolve/open through the same open_link/
                            resolve_link_target. See TODO/viewer.md.
  explorer/user_menu/    —   F2 -- per-directory script menu, native
                            format LitastumMenu.toml (parse/ -- Far
                            Manager's own FarMenu.ini nested-block DSL
                            (dsl.rs, used only to *read* a FarMenu.ini
                            for one-time porting), the full Far !...!
                            macro set + litastum's own {{...}} macros
                            (substitution.rs -- named to avoid colliding
                            with Far's own, much bigger and unrelated
                            macro-recording feature, not implemented
                            here), and the !?Label?Default!/
                            {{prompt:...}} placeholder (prompts.rs);
                            toml_format.rs: litastum's own serde-backed
                            TOML shape + MenuItem conversion; state.rs:
                            file resolution, porting on confirmation,
                            nested navigation over one canonical tree
                            (not a stack of clones -- an edit at any
                            depth has to reach the same tree that gets
                            persisted), add/remove an item + persist,
                            the prompt-collection state; input.rs: key
                            handling, hands finished commands off to
                            command_line::run_shell_command_lines)

editor.rs               — F4 built-in editor, backed by `edtui`
  editor/editor/         —   Editor: open/save/view, search (search.rs),
                              word-select-touch (word_select_touch.rs)
  editor/editor_keymap/  —   KeyEvent -> edtui action table + our own
                              intercepts (search box, word-select)
  editor/bindings/       —   hand-rolled motions edtui's own action
                              table can't express (word-wise selection,
                              line-wrap arrow movement, shift-select)
  editor/syntax/         —   bundled .sublime-syntax grammars syntect's
                              default set is missing (PowerShell, INI, ...)
  editor/clipboard.rs    —   OsClipboardBridge (arboard <-> edtui)
  editor/word_highlight.rs — same-word occurrence highlighting
  editor/find_history.rs —   Ctrl+F search-box ghost-text suggestion history

theming.rs              — Theme, color-scheme loading/persistence,
  theming/theme.rs       —   the resolved Theme struct itself
  theming/scheme.rs      —   ColorScheme: parses Windows Terminal JSON
                              scheme files, derives Theme + syntect Theme
  theming/config/        —   config.json read/write (interface_theme,
                              editor_theme, active_shell, popup_style),
                              theme file search (config dir + ./themes/)
  theming/menu.rs        —   F9 top menu (MainMenu, MenuLevel, dispatch)
  theming/theme_menu.rs  —   F9 -> Options -> Color schemes picker
  theming/popup_style.rs —   PopupStyle (Classic/Rounded)
  theming/popup_style_menu.rs — F9 -> Options -> UI picker

command_line.rs         — the always-live Far-style command line
  command_line/browsing/ —   Mode::Browsing key dispatch (the other big
                              chokepoint alongside explorer::command::execute);
                              also owns run_shell_command_lines, the shared
                              "suspend the TUI, run N lines through the
                              active shell" primitive explorer::user_menu
                              reuses for its own item execution
  command_line/completion.rs — Tab path completion, cycling
  command_line/history.rs —  command history, Alt+F8 popup, ghost-text
                              autosuggestion
  command_line/shell.rs  —   Ctrl+P shell-profile picker

text_field.rs           — shared cursor/selection editing (transfer
                           destination field, command line's selection)

ui/mod.rs               — pure(ish) rendering: App -> ratatui widgets
  ui/popup.rs            —   shared popup chrome (draw_frame, key_pill,
                              separator), style-aware (Classic/Rounded);
                              also selected_row_style/selected_text_style
                              (theme.current_row_bg + theme.selection_text
                              override) -- every popup's own selected-row/
                              text-selection styling builds on these
                              instead of repeating the same Style literal
  ui/preview.rs          —   shared full-panel-preview chrome
                              (draw_preview_frame, file_title) --
                              image_preview.rs and markdown_preview.rs
                              both build on this instead of each
                              constructing their own accent-bordered
                              Block, same "shared primitive" pattern
                              popup.rs already established for popups
  ui/{menu,shell,drive_menu,find_file,confirm,theme_menu,
      popup_style_menu,editor_find,command_line,user_menu}.rs
                         —   one rendering module per popup/overlay
```

`vfs.rs` (archives/SFTP, roadmap stage 6) and `script.rs` (`rhai`
scripting, roadmap stage 4) don't exist yet — still several stages out,
not designed further here than `.claude/rules/litastum-roadmap.md`
already does.

## Data flow

```
crossterm::event::read()
        |
        v
   main.rs::handle_event()  -- dispatches on app.mode to one of:
        explorer::{handle_confirm_delete_key, handle_confirm_transfer_key,
                   handle_find_file_key, handle_drive_menu_key}
        theming::{handle_main_menu_key, handle_theme_menu_key,
                  handle_popup_style_menu_key}
        command_line::{handle_shell_menu_key, handle_history_key,
                       handle_browsing_key}
        editor::{handle_editor_key, handle_confirm_discard_key}
        |
        v
   explorer::command::execute(Command, &mut App)  -- the shared
   chokepoint every *browsing*-mode key (and later, scripts) funnels
   filesystem/mode changes through; other modes own their own
   mutation directly in their handle_*_key (rename/delete/theme-apply/
   etc. don't need a shared Command enum the way panel navigation does)
        |
        v
   ui::draw(frame, &mut App)  -- theme.rs supplies styles, app.popup_style
   picks Classic vs Rounded chrome for every popup
```

Each non-`Browsing` `Mode` (there are a dozen — see `app.rs::Mode`) is
a self-contained `{State struct, Command enum, resolve(), handle_*_key,
draw_*}` group, split across its owning module (`theming::theme_menu`,
`explorer::find_file`, ...) and a matching `ui::*` rendering module —
the same shape `explorer::keymap`/`explorer::command` established for
panel navigation, just scoped to one popup's own key handling instead
of a codebase-wide chokepoint. `ui::draw` itself stays close to a pure
function of `&App` for the browsing panels; the built-in editor is the
one real exception (`edtui::EditorView`'s own render step needs
`&mut EditorState` even just to draw, not a choice on this project's
side), which is why `ui::draw` takes `app: &mut App` overall rather
than `&App`.

## Panel: column-major layout

The file grid fills column 1 top-to-bottom before column 2 (Far
Manager's own "Brief" view convention) — Up/Down flow through the whole
grid in that column-major order (reaching the bottom of a column
continues into the top of the next one), Left/Right jump a whole column
at once, same row.

```rust
impl Panel {
    fn rows(&self) -> usize { ... } // entries.len().div_ceil(columns), clamped to what's visible

    pub fn move_up(&mut self)    { self.selected = self.selected.saturating_sub(1) }
    pub fn move_down(&mut self)  { if self.selected + 1 < self.entries.len() { self.selected += 1 } }
    pub fn move_left(&mut self)  { ... } // jumps back by rows(), paginating across pages at the edge
    pub fn move_right(&mut self) { ... } // mirror of move_left
}
```

`columns` is derived each frame by `ui::draw_panel` from
`area.width / MIN_COLUMN_WIDTH` and written back onto `Panel` so
`move_left`/`move_right` (which run from `explorer::command`, outside
`ui`) agree with what was actually rendered. `scroll_offset` and
`visible_rows` (added once real directories exceeded what one screen
could show) keep the currently-visible page in sync with the cursor —
see `Panel::ensure_selected_visible_paginated`'s own doc comment for
why this needs real tracked state rather than handing every entry to a
plain `List` and trusting it to scroll on its own (it doesn't).
`marks: HashSet<usize>` (`panel/marks.rs`) is the Far Manager-style
multi-select F5/F6/F8 act on when non-empty, falling back to the
cursor entry otherwise.

## Popup chrome: Classic vs Rounded

Every popup (F9 menu, color-scheme/UI pickers, Ctrl+P shell picker,
Alt+F1/F2 drive menu, Find file, the F5/F6/F8 confirm prompts) renders
through `ui::popup::draw_frame(frame, area, theme, style, title, width,
height)`, which returns the caller's own content `Rect` regardless of
which `PopupStyle` (`theming::PopupStyle`) is active:

- `Classic` — plain square `Block::borders(ALL)`, title baked into the
  border, no interior padding. The look every popup used before
  `ui/popup.rs` existed.
- `Rounded` — `BorderType::Rounded`, uniform padding, `title` rendered
  as the frame's own first content line with a separator under it.

Both are permanent, user-selectable options (F9 -> Options -> UI,
`theming::popup_style_menu`) rather than one converging on the other —
see `.claude/rules/litastum-popup-design.md` for `Rounded`'s own
settled fill/corner-glyph history. `ui/editor_find.rs`'s Ctrl+F search
box deliberately opts out of this shared chrome entirely (reported too
large/labeled with it) — see its own doc comment.

## What's still out of scope

- `vfs.rs` (archives, SFTP — roadmap stage 6) and `script.rs` (`rhai`
  scripting — roadmap stage 4): not started. `explorer::command::execute`
  is already the intended chokepoint scripts will emit `Command` values
  through, per `.claude/rules/litastum-stack.md`'s constraint against
  scripts touching the filesystem/process directly.
- Multi-select exists (`panel/marks.rs`) but only for
  Copy/Move/Delete/Rename — no other command reads `marked` yet.
- Diff view / conflict resolver — placeholder only, see
  `TODO/next-up.md`.
