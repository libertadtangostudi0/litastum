use color_eyre::eyre::Result;

use crate::app::{App, Mode, PendingDelete, PendingTransfer, TransferOp};
use crate::editor::Editor;
use crate::theming::MainMenu;
use super::keymap::Command;


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
/// `confirm::handle_confirm_delete_key` for the actual filesystem call.
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


#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::theming::Theme;

    /// A fresh scratch directory under the OS temp dir, unique per test
    /// (same pattern as `fs_ops.rs`/`panel.rs`'s own scratch helpers).
    fn scratch_dir() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litastum-command-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Both panels rooted at a real scratch directory containing one
    /// real file `name`, with the active panel's cursor already on it
    /// (never on the synthetic `..` entry).
    fn app_with_selected_file(name: &str) -> App {
        let dir = scratch_dir();
        fs::write(dir.join(name), b"data").expect("write scratch file");
        let mut app = App::new(dir, Theme::dark(), None).expect("build app");
        let idx = app.panels[0].entries.iter().position(|e| e.name == name).expect("entry listed");
        app.panels[0].selected = idx;
        app
    }

    #[test]
    fn request_delete_targets_the_selected_entry() {
        let mut app = app_with_selected_file("victim.txt");
        let expected_path = app.panels[0].path.join("victim.txt");

        request_delete(&mut app);

        let Mode::ConfirmDelete(pending) = &app.mode else {
            panic!("expected Mode::ConfirmDelete");
        };
        assert_eq!(pending.path, expected_path);
        assert_eq!(pending.name, "victim.txt");
        assert!(!pending.is_dir);
    }

    #[test]
    fn request_delete_on_dotdot_does_nothing() {
        let mut app = app_with_selected_file("victim.txt");
        app.panels[0].selected = 0; // ".." is always entry 0 when a parent exists

        request_delete(&mut app);

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn request_delete_on_an_empty_panel_does_nothing() {
        let mut app = App::new(scratch_dir(), Theme::dark(), None).expect("build app");

        request_delete(&mut app);

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn request_transfer_defaults_the_destination_to_the_other_panel() {
        let mut app = app_with_selected_file("source.txt");
        let other_dir = app.panels[1].path.clone();

        request_transfer(&mut app, TransferOp::Copy);

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("expected Mode::ConfirmTransfer");
        };
        assert_eq!(pending.operation, TransferOp::Copy);
        assert_eq!(pending.destination, other_dir.join("source.txt").to_string_lossy());
        assert_eq!(pending.cursor, pending.destination.chars().count(), "cursor starts at the end");
        assert_eq!(pending.selection_anchor, None);
    }

    #[test]
    fn request_transfer_move_sets_the_move_operation() {
        let mut app = app_with_selected_file("source.txt");

        request_transfer(&mut app, TransferOp::Move);

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("expected Mode::ConfirmTransfer");
        };
        assert_eq!(pending.operation, TransferOp::Move);
    }

    #[test]
    fn request_transfer_on_dotdot_does_nothing() {
        let mut app = app_with_selected_file("source.txt");
        app.panels[0].selected = 0;

        request_transfer(&mut app, TransferOp::Copy);

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn request_rename_defaults_the_destination_to_the_same_directory() {
        let mut app = app_with_selected_file("source.txt");
        let same_dir = app.panels[0].path.clone();

        request_rename(&mut app);

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("expected Mode::ConfirmTransfer");
        };
        assert_eq!(pending.operation, TransferOp::Move);
        assert_eq!(pending.destination, same_dir.join("source.txt").to_string_lossy());
    }

    #[test]
    fn request_rename_places_the_cursor_right_before_the_filename() {
        let mut app = app_with_selected_file("source.txt");

        request_rename(&mut app);

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("expected Mode::ConfirmTransfer");
        };
        let expected = pending.destination.chars().count() - "source.txt".chars().count();
        assert_eq!(pending.cursor, expected);
        assert_eq!(&pending.destination[pending.destination.char_indices().nth(pending.cursor).unwrap().0..], "source.txt");
    }

    /// Regression guard for the cursor-position arithmetic in
    /// `request_rename` (`destination.chars().count() -
    /// entry.name.chars().count()`), which relies on `to_string_lossy()`
    /// leaving the trailing filename's char count untouched. A
    /// multi-byte-but-still-valid-UTF-8 name (unlike the parent
    /// directory, which on a real OS could contain non-UTF-8 bytes that
    /// `to_string_lossy()` would replace and shrink/grow) is the case
    /// this could break under if that assumption were ever wrong.
    #[test]
    fn request_rename_handles_a_multibyte_filename_without_underflow() {
        let mut app = app_with_selected_file("café_résumé.txt");

        request_rename(&mut app);

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("expected Mode::ConfirmTransfer");
        };
        let expected = pending.destination.chars().count() - "café_résumé.txt".chars().count();
        assert_eq!(pending.cursor, expected);
    }
}
