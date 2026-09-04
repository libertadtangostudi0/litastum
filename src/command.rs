use color_eyre::eyre::Result;

use crate::app::{App, Mode, PendingDelete, PendingTransfer, TransferOp};
use crate::editor::Editor;
use crate::keymap::Command;
use crate::menu::MainMenu;


/// Executes a resolved `Command` against the app state. This is the one
/// chokepoint keyboard input goes through — and, per the roadmap's
/// stage 4, where scripts will funnel their commands too, instead of
/// touching the filesystem/process directly.
pub fn execute(command: Command, app: &mut App) -> Result<()> {
    match command {
        Command::MoveUp => app.active_panel().move_up(),
        Command::MoveDown => app.active_panel().move_down(),
        Command::MoveLeft => app.active_panel().move_left(),
        Command::MoveRight => app.active_panel().move_right(),
        Command::EnterSelected => app.active_panel().enter_selected()?,
        Command::ToggleActive => app.toggle_active(),
        Command::EditSelected => open_editor(app),
        Command::OpenMenu => app.mode = Mode::MainMenu(MainMenu::open()),
        Command::CopySelected => request_transfer(app, TransferOp::Copy),
        Command::MoveSelected => request_transfer(app, TransferOp::Move),
        Command::RenameSelected => request_rename(app),
        Command::DeleteSelected => request_delete(app),
        Command::Quit => app.should_quit = true,
    }
    Ok(())
}


/// Opens the file under the cursor in the built-in editor (`editor.rs`,
/// backed by `edtui`). Does nothing for directories, and for files that
/// fail to load as UTF-8 text (binary files aren't supported yet — see
/// `TODO.md`) rather than crashing the app.
fn open_editor(app: &mut App) {
    let Some(path) = app.active_panel().selected_path() else {
        return;
    };
    if path.is_dir() {
        return;
    }

    let syntax_theme = app.syntax_theme.clone();
    if let Ok(editor) = Editor::open(path, syntax_theme) {
        app.mode = Mode::Editing(editor);
    }
}


/// F8: opens the "delete this?" prompt (`Mode::ConfirmDelete`) for the
/// entry under the cursor. Does nothing for `..` (there's nothing
/// sensible to delete) or an empty panel — never deletes directly, see
/// `main.rs::handle_confirm_delete_key` for the actual filesystem call.
fn request_delete(app: &mut App) {
    let panel = app.active_panel();
    let Some(entry) = panel.current() else {
        return;
    };
    if entry.name == ".." {
        return;
    }

    let pending = PendingDelete {
        path: panel.path.join(&entry.name),
        name: entry.name.clone(),
        is_dir: entry.is_dir,
    };
    app.mode = Mode::ConfirmDelete(pending);
}


/// F5/F6: opens the "copy/move to?" prompt (`Mode::ConfirmTransfer`)
/// for the entry under the cursor, pre-filled with the *other* panel's
/// directory as the destination — Far Manager's own F5/F6 default.
/// Does nothing for `..` or an empty panel, same as `request_delete`.
fn request_transfer(app: &mut App, operation: TransferOp) {
    let destination_dir = app.panels[1 - app.active].path.clone();

    let panel = app.active_panel();
    let Some(entry) = panel.current() else {
        return;
    };
    if entry.name == ".." {
        return;
    }

    let destination = destination_dir.join(&entry.name).to_string_lossy().into_owned();
    let cursor = destination.chars().count();
    let pending = PendingTransfer {
        operation,
        source: panel.path.join(&entry.name),
        name: entry.name.clone(),
        is_dir: entry.is_dir,
        destination,
        cursor,
        selection_anchor: None,
    };
    app.mode = Mode::ConfirmTransfer(pending);
}


/// `Shift+F6`: opens the same `Mode::ConfirmTransfer` prompt as
/// `request_transfer(_, Move)`, but the destination defaults to the
/// entry's own directory instead of the other panel's — so confirming
/// with the destination untouched is a no-op, and the actual use case
/// (editing the name before confirming) renames in place via the same
/// `fs_ops::move_entry` call `main.rs` already makes for a real
/// cross-panel move. The cursor starts right after the directory part
/// (at the start of the filename, not the end of the whole path) so
/// typing immediately edits the name — the whole point of this
/// binding — without needing `Home`/`Ctrl+Left` first.
fn request_rename(app: &mut App) {
    let panel = app.active_panel();
    let Some(entry) = panel.current() else {
        return;
    };
    if entry.name == ".." {
        return;
    }

    let path = panel.path.join(&entry.name);
    let destination = path.to_string_lossy().into_owned();
    let cursor = destination.chars().count() - entry.name.chars().count();
    let pending = PendingTransfer {
        operation: TransferOp::Move,
        source: path,
        name: entry.name.clone(),
        is_dir: entry.is_dir,
        destination,
        cursor,
        selection_anchor: None,
    };
    app.mode = Mode::ConfirmTransfer(pending);
}
