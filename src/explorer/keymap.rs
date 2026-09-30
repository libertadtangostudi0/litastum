/// A panel-level action. Bound to keys in the browser's binding table
/// (`command_line::browsing::bindings`), and the typed value scripting
/// (stage 4 of the roadmap) can emit instead of a raw key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    /// `Enter` — descends into the directory under the cursor (or its
    /// parent, for `..`), same as always; on a *file*, opens it in the
    /// built-in editor instead (`explorer::command::open_editor`, the
    /// same thing `F4`/`EditSelected` does) rather than doing nothing,
    /// per an explicit request that plain `Enter` shouldn't be a no-op
    /// on a file.
    EnterSelected,
    /// `Shift+Enter`: a directory opens in the OS file manager
    /// (`explorer::system_open`); a file opens in the editor, like `Enter`.
    OpenInFileManager,
    ToggleActive,
    EditSelected,
    /// `F3`: previews the entry -- an image in the right panel
    /// (`Mode::ImagePreview`), or a `.md` file in the editor with a live
    /// preview beside it. No-op otherwise (`TODO/viewer.md`).
    PreviewSelected,
    /// `F2` — Far Manager's own "user menu" (`explorer::user_menu`): a
    /// per-directory list of shell-command shortcuts, read from
    /// `LitastumMenu.toml`. If only a compatible `FarMenu.ini` exists,
    /// offers to port it first (`Overlay::ConfirmPortFarMenu`); if neither
    /// exists, creates an empty `LitastumMenu.toml` there and opens it
    /// in the built-in editor instead of browsing an empty menu.
    OpenUserMenu,
    /// F9 — opens the top menu (`menu.rs`), currently `Settings` →
    /// `Color schemes` (`theme_menu.rs`); a minimal analog of Far
    /// Manager's F9 menu, scoped to just that path for now.
    OpenMenu,
    /// F5 — asks to copy the entry under the cursor into the *other*
    /// panel's directory (`Overlay::ConfirmTransfer`), Far Manager-style.
    CopySelected,
    /// F6 — same as `CopySelected` but moves instead of copying.
    MoveSelected,
    /// `Shift+F6` — Far Manager's own "Rename or move" binding: opens
    /// the same `Overlay::ConfirmTransfer` prompt as `MoveSelected`, but
    /// defaulting the destination to the entry's *own* directory
    /// (rather than the other panel's) so editing just the trailing
    /// name renames it in place.
    RenameSelected,
    /// F8 — asks to delete the entry under the cursor
    /// (`Overlay::ConfirmDelete`), never deletes directly. Matches Far
    /// Manager's own F8 binding.
    DeleteSelected,
    /// `Shift+A` (only on an empty command line -- see the binding table)
    /// — marks every entry in the active panel except `..`
    /// (`panel/marks.rs::select_all`).
    SelectAll,
    /// `Shift+Up` — toggles the mark on the entry under the cursor,
    /// then moves up one row (`panel/marks.rs::toggle_mark_move_up`).
    MarkMoveUp,
    /// `Shift+Down` — mirror of `MarkMoveUp`.
    MarkMoveDown,
    /// `Shift+Left` — toggles the mark on every entry the existing
    /// paginated column jump (`Panel::move_left`) crosses
    /// (`panel/marks.rs::toggle_mark_move_left`).
    MarkMoveLeft,
    /// `Shift+Right` — mirror of `MarkMoveLeft`.
    MarkMoveRight,
    /// `Ctrl+U` — swaps the two panels' full contents (path, entries,
    /// cursor, scroll, marks — `App::swap_panels`), keeping keyboard
    /// focus on the same screen *side*, exactly like real Far
    /// Manager's own Ctrl+U.
    SwapPanels,
    Quit,
}
