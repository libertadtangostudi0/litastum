# litastum — architecture sketch

Draft architecture derived from the planning chat + the Far Manager UI
mockup (`far_manager_reference_color2_columns.html`, dark GitHub-inspired
theme, 2-column panels, arrow-key nav across columns). This extends the
current scaffold (stage 0-2 of `CLAUDE.md`'s roadmap) toward stage 3+
without a rewrite — same crates, same module split, new modules added
where the mockup needs behavior the scaffold doesn't have yet.

## Module map

```
main.rs      — terminal setup/teardown, event loop, wires keymap -> command
app.rs       — App: owns panels, active index, mode, should_quit
panel.rs     — Panel: cwd, entries, cursor, column-aware navigation
ui.rs        — pure rendering: App -> ratatui widgets (no state mutation)
theme.rs     — color/style constants, one place matching the mockup's CSS
keymap.rs    — KeyCode -> Command mapping
command.rs   — Command enum + execution (owns the F4 editor-shellout etc.)
config.rs    — LATER (stage 5): on-disk config via `directories`, live reload via `notify`
editor.rs    — LATER (stage 3): built-in editor widget (ratatui-textarea/edtui)
vfs.rs       — LATER (stage 6): trait over {std::fs, zip/tar, sftp} so Panel doesn't care
script.rs    — LATER (stage 4): rhai host, Message/Command in — no direct fs/process access
```

`panel.rs` and `ui.rs` already being separate from `app.rs`/`main.rs` is
the right split — the additions below slot into it rather than replacing
it.

## Data flow

```
crossterm::event::read()
        |
        v
   main.rs: handle_event()  -- keymap.rs resolves KeyCode -> Command
        |
        v
   command.rs: execute(Command, &mut App)  -- mutates App/Panel/editor state
        |
        v
   ui.rs: draw(&App)  -- theme.rs supplies styles, pure read of App state
```

`ui.rs` stays a pure function of `&App` for the browsing panels (no
`io::Result`, no mutation). **Exception**: `draw` takes `app: &mut App`
overall, because the built-in editor's `edtui::EditorView` tracks scroll
position as part of rendering and needs `&mut EditorState` even just to
draw — not a design choice on our side, `edtui`'s own render step
mutates view state. Everything outside the `Mode::Editing`/
`Mode::ConfirmDiscard` branches still only reads `app`.

## Panel: column-major layout (the mockup's core UI change)

The mockup's grid (`grid-auto-flow:column`) fills column 1 top-to-bottom
before column 2. Up/Down flow through the whole grid in that same
column-major order — reaching the bottom of a column continues into the
top of the next one, rather than stopping there — and Left/Right jump
directly across columns, same row. (An earlier version of this doc had
Up/Down clamp at each column's edge, matching the mockup's own arrow-key
JS; that felt wrong once actually used in a terminal and was corrected
after implementation.)

`Panel` needs:

```rust
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,   // index into entries, unchanged
    pub columns: usize,    // NEW: computed by ui.rs from area width, written back each frame
}
```

Column-major index math (row-major storage, column-major display, same
trick the mockup's JS uses via `data-row`/`data-col`):

```rust
impl Panel {
    fn rows(&self) -> usize { self.entries.len().div_ceil(self.columns) }

    // entries is already stored in column-major order, so a plain linear
    // step already flows correctly from one column into the next.
    pub fn move_up(&mut self)    { self.selected = self.selected.saturating_sub(1) }
    pub fn move_down(&mut self)  { if self.selected + 1 < self.entries.len() { self.selected += 1 } }

    // Left/Right jump a whole column (same row), independent of Up/Down.
    pub fn move_left(&mut self)  { self.selected = self.selected.saturating_sub(self.rows()) }
    pub fn move_right(&mut self) { let n = self.selected + self.rows();
                                       if n < self.entries.len() { self.selected = n } }
}
```

`columns` is derived, not user-set: `ui.rs` computes it each frame from
`area.width / min_column_width` (mockup uses a fixed 2, but the panel
width is fixed there too — real terminal panels resize, so this should
scale: 1 column below some width threshold, 2+ above). Store it back on
`Panel` so `move_up`/`move_down` — which run from `main.rs`, outside
`ui.rs` — see the same column count the last frame rendered with.

Left/Right currently means "switch active panel" (there's no such binding
yet in the scaffold, `Tab` does it) — decide: either Left/Right always
means "move across columns within the panel, clamped at the edge" (matches
the mockup, which never crosses into the other panel), or reserve it for
switching panels and use a different key for cross-column movement inside
one panel. The mockup's own JS clamps at panel edges, so recommend the
former: Left/Right stays column movement inside the active panel, Tab
keeps switching panels.

## Theming

`theme.rs` becomes the single source of truth, values lifted straight
from the mockup's inline CSS:

```rust
pub struct Theme {
    pub bg: Color,          // #0d1117
    pub surface: Color,     // #161b22
    pub border: Color,      // #30363d
    pub border_dim: Color,  // #21262d
    pub text: Color,        // #e6edf3
    pub text_dim: Color,    // #8b949e
    pub text_muted: Color,  // #6e7681
    pub accent: Color,      // #58a6ff  — focus / cursor row / F-key labels
    pub success: Color,     // #3fb950
    pub danger: Color,      // #f85149  — F8 Delete only
    pub warning: Color,     // #d29922  — marked/selected files
}
```

`ratatui::style::Color::Rgb` takes these directly (crossterm backend
supports truecolor on all three target platforms this project cares
about). `ui.rs` takes `&Theme` alongside `&App` — pass it through `draw()`
rather than making it a global, so tests/snapshots can swap themes later
without statics.

Maps onto the mockup's states directly:
- active panel border → `accent`, inactive → `border`
- current row (focused panel) → `accent` at low alpha + left border —
  ratatui has no alpha; approximate with a solid `bg`-blended `Rgb`
  precomputed in `theme.rs`, or just invert fg/bg like the current
  scaffold's `Cyan`/`Black` selection style
- current row (unfocused panel) → `border` left-border only, no fill
  (mockup's `.fm-panel:not(.fm-active) .fm-current`)
- marked files → `warning` at low alpha + left border (multi-select
  doesn't exist in `Panel` yet — needs a `marked: HashSet<usize>` field
  when F5/F6/F8 gain real "act on selection" semantics instead of
  "act on cursor")
- F8 label → `danger`, all other F-keys → `accent` (matches mockup
  exactly: only Delete is red)

## Command layer

Right now `main.rs::handle_event` both resolves keys *and* executes them
(`open_editor_for_selection` is inlined there). Splitting these two
concerns means the growing key table (F1-F10, Left/Right, future
Copy/Move/Delete confirmation flows) doesn't keep bloating `main.rs`:

```rust
// keymap.rs
pub enum Command {
    MoveUp, MoveDown, MoveLeft, MoveRight,
    EnterSelected, ToggleActive,
    EditSelected, Quit,
    // later: CopySelected, MoveSelected, DeleteSelected(needs confirm), NewFolder, ...
}
pub fn resolve(key: KeyCode) -> Option<Command> { ... }

// command.rs
pub fn execute(cmd: Command, app: &mut App, terminal: &mut Terminal<...>) -> Result<()> { ... }
```

This is also where the roadmap's stage-4 scripting plugs in later: `rhai`
scripts emit the *same* `Command` values instead of touching `fs`/process
directly (per `CLAUDE.md`'s existing constraint) — `command.rs::execute`
becomes the one chokepoint both keyboard and scripts go through.

## What this sketch deliberately leaves out

- Multi-select (`marked` set) — needed for real Copy/Move/Delete-on-
  selection, but the mockup only shows the *visual* state, not the
  interaction; add when F5/F6/F8 stop being cursor-only.
- Status bar / command-line input (mockup's bottom `cargo build --release`
  line) — cosmetic only for now; wiring a real shell there is a separate
  , larger feature (subprocess + PTY concerns), not part of this pass.
- `vfs.rs`, `editor.rs`, `config.rs`, `script.rs` — stubbed as module
  names above for where they'll live per `CLAUDE.md`'s staged roadmap;
  not designed here since they're several stages out.

## Suggested implementation order

1. `theme.rs` — pure data, zero risk, unlocks visually matching the
   mockup immediately in the existing single-column `List` renderer.
2. Column-major `Panel` nav + `ui.rs` grid rendering (replace `List`
   with a manual `Layout`/`Table`-based grid, since `ratatui::List` can't
   do column-major fill).
3. `keymap.rs` + `command.rs` extraction from `main.rs` — mechanical
   refactor, no behavior change, makes step 4 additive instead of
   invasive.
4. Wire the new Left/Right column movement and Tab panel-switch through
   the new command layer.
