use crossterm::event::{KeyCode, KeyEvent};


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
    /// `Shift+Enter` — on a directory, opens it in the OS's own file
    /// manager (`explorer::system_open`) instead of navigating the
    /// panel into it, analogous to Far Manager's own external-open
    /// bindings (the same thing a real double-click on a folder would
    /// do); on a *file*, opens it in the built-in editor, exactly like
    /// plain `Enter` does (see `EnterSelected` above) rather than
    /// handing it to the OS too — requested directly, so `Shift+Enter`
    /// and plain `Enter` behave identically on a file and differ only
    /// on a directory.
    OpenInFileManager,
    ToggleActive,
    EditSelected,
    /// `F3` -- previews the entry under the cursor: an image
    /// (`.jpg`/`.jpeg`/`.png`/`.bmp` --
    /// `explorer::image_preview::is_supported_image`, `Mode::ImagePreview`)
    /// in the *right* panel, or a `.md`/`.markdown` file opened for
    /// editing in the *left* panel with a live rendered preview linked
    /// alongside it in the right (`explorer::markdown_preview::is_markdown_file`,
    /// `explorer::markdown_preview::open_edit_preview`, `App::markdown_edit_preview`).
    /// A no-op for anything else (a directory, an unsupported file, an
    /// undecodable/unreadable one) -- see `TODO/viewer.md` for what F3
    /// is eventually meant to cover beyond these two.
    PreviewSelected,
    /// `F2` — Far Manager's own "user menu" (`explorer::user_menu`): a
    /// per-directory list of shell-command shortcuts, read from
    /// `LitastumMenu.toml`. If only a compatible `FarMenu.ini` exists,
    /// offers to port it first (`Mode::ConfirmPortFarMenu`); if neither
    /// exists, creates an empty `LitastumMenu.toml` there and opens it
    /// in the built-in editor instead of browsing an empty menu.
    OpenUserMenu,
    /// F9 — opens the top menu (`menu.rs`), currently `Settings` →
    /// `Color schemes` (`theme_menu.rs`); a minimal analog of Far
    /// Manager's F9 menu, scoped to just that path for now.
    OpenMenu,
    /// F5 — asks to copy the entry under the cursor into the *other*
    /// panel's directory (`Mode::ConfirmTransfer`), Far Manager-style.
    CopySelected,
    /// F6 — same as `CopySelected` but moves instead of copying.
    MoveSelected,
    /// `Shift+F6` — Far Manager's own "Rename or move" binding: opens
    /// the same `Mode::ConfirmTransfer` prompt as `MoveSelected`, but
    /// defaulting the destination to the entry's *own* directory
    /// (rather than the other panel's) so editing just the trailing
    /// name renames it in place.
    RenameSelected,
    /// F8 — asks to delete the entry under the cursor
    /// (`Mode::ConfirmDelete`), never deletes directly. Matches Far
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


/// The choice on the "delete this?" prompt (`Mode::ConfirmDelete`) —
/// same Y/N/Esc shape as the editor's `ConfirmDiscardCommand`
/// (`editor_keymap.rs`), kept as its own type rather than shared since
/// this one lives in browsing mode, not the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDeleteCommand {
    Confirm,
    Cancel,
    /// Anything else — the prompt only understands these two answers.
    Ignore,
}


/// Resolves a raw key press on the delete-confirmation prompt.
pub fn resolve_confirm_delete(key: KeyEvent) -> ConfirmDeleteCommand {
    match key.code {
        KeyCode::Char('y' | 'Y') => ConfirmDeleteCommand::Confirm,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => ConfirmDeleteCommand::Cancel,
        _ => ConfirmDeleteCommand::Ignore,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    #[test]
    fn y_or_uppercase_y_confirms_delete() {
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('y'))), ConfirmDeleteCommand::Confirm);
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('Y'))), ConfirmDeleteCommand::Confirm);
    }

    #[test]
    fn n_or_esc_cancels_delete() {
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('n'))), ConfirmDeleteCommand::Cancel);
        assert_eq!(resolve_confirm_delete(key(KeyCode::Esc)), ConfirmDeleteCommand::Cancel);
    }

    #[test]
    fn other_keys_are_ignored_on_the_delete_prompt() {
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('x'))), ConfirmDeleteCommand::Ignore);
        assert_eq!(resolve_confirm_delete(key(KeyCode::Enter)), ConfirmDeleteCommand::Ignore);
    }
}
