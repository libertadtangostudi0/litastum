//! `command.rs` split into one file per concern once it passed the
//! project's own ~500-line decomposition threshold ([[code-conventions]])
//! -- 716 lines, mixing the central `execute` dispatcher with every
//! command's own implementation. `execute` itself stays here since it
//! needs to see every submodule's handlers; everything else moves to
//! `open.rs` (`F2`/`F3`/`F4`/`Enter`/`Shift+Enter` -- opening something)
//! or `transfer.rs` (`F5`/`F6`/`F8`/`Shift+F6` -- copy/move/delete/
//! rename). Only `execute` was ever reachable from outside this module
//! (`explorer.rs`'s own `pub use command::execute;`) -- every other
//! function here was already a private implementation detail, so
//! nothing needed re-exporting to keep external code working.

mod open;
mod transfer;

use color_eyre::eyre::Result;

use crate::app::{App, Mode, TransferOp};
use crate::theming::MainMenu;
use super::keymap::Command;

use open::{current_entry_is_dir, open_editor, open_directory_in_file_manager, open_user_menu, preview_selected};
use transfer::{request_delete, request_rename, request_transfer};

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
        Command::EnterSelected => {
            if current_entry_is_dir(app) {
                app.active_panel().enter_selected()?
            } else {
                open_editor(app)
            }
        }
        Command::OpenInFileManager => {
            if current_entry_is_dir(app) {
                open_directory_in_file_manager(app)
            } else {
                open_editor(app)
            }
        }
        Command::ToggleActive => app.toggle_active(),
        Command::EditSelected => open_editor(app),
        Command::PreviewSelected => preview_selected(app),
        Command::OpenUserMenu => open_user_menu(app),
        Command::OpenMenu => app.mode = Mode::MainMenu(MainMenu::open()),
        Command::CopySelected => request_transfer(app, TransferOp::Copy),
        Command::MoveSelected => request_transfer(app, TransferOp::Move),
        Command::RenameSelected => request_rename(app),
        Command::DeleteSelected => request_delete(app),
        Command::SelectAll => app.active_panel().select_all(),
        Command::MarkMoveUp => app.active_panel().toggle_mark_move_up(),
        Command::MarkMoveDown => app.active_panel().toggle_mark_move_down(),
        Command::MarkMoveLeft => app.active_panel().toggle_mark_move_left(),
        Command::MarkMoveRight => app.active_panel().toggle_mark_move_right(),
        Command::Quit => app.should_quit = true,
    }
    Ok(())
}


/// Shared by both submodules' own `#[cfg(test)] mod tests` -- a plain,
/// non-`pub` item here is already visible to every descendant module,
/// same pattern `state/mod.rs::scratch_dir`/`input/mod.rs::scratch_dir`
/// already use.
#[cfg(test)]
fn scratch_dir() -> std::path::PathBuf {
    crate::test_support::unique_scratch_dir("command")
}

/// Both panels rooted at a real scratch directory containing one
/// real file `name`, with the active panel's cursor already on it
/// (never on the synthetic `..` entry).
#[cfg(test)]
fn app_with_selected_file(name: &str) -> App {
    use std::fs;
    let dir = scratch_dir();
    fs::write(dir.join(name), b"data").expect("write scratch file");
    let mut app = crate::test_support::test_app(dir);
    let idx = app.panels[0].entries.iter().position(|e| e.name == name).expect("entry listed");
    app.panels[0].selected = idx;
    app
}
