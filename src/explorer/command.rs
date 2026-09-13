use std::path::PathBuf;

use color_eyre::eyre::Result;
use tracing::warn;

use crate::app::{App, DeleteEntry, Mode, PendingDelete, PendingTransfer, TransferOp, TransferSource};
use crate::editor::Editor;
use crate::theming::MainMenu;
use super::keymap::Command;
use super::{image_preview, system_open, user_menu, Panel};


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
        Command::PreviewSelected => image_preview::open_preview(app),
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


/// `F2`: opens the active panel's own user menu. Three cases, per
/// `user_menu::resolve_menu`:
/// - A `FarMenu.ini` exists -- ask before touching anything
///   (`Mode::ConfirmPortFarMenu`), rather than converting or reading it
///   silently; `handle_confirm_port_far_menu_key` does the actual port
///   once confirmed. Takes priority even over an already-existing
///   `LitastumMenu.toml` (`resolve_menu`'s own doc comment) -- the same
///   startup check runs once in `main.rs`, so this same prompt can also
///   appear before `F2` is ever pressed.
/// - Only `LitastumMenu.toml` exists -- browse it directly.
/// - Neither exists -- creates an empty `LitastumMenu.toml` right there
///   and opens it in the built-in editor instead of browsing an empty
///   popup with nothing in it to select. A failed creation (read-only
///   directory, permissions, ...) is a silent no-op, same as any other
///   "couldn't act on this" case in this codebase.
fn open_user_menu(app: &mut App) {
    let dir = app.active_panel().path.clone();
    match user_menu::resolve_menu(&dir) {
        user_menu::MenuFile::Own(items) => {
            app.mode = Mode::UserMenu(user_menu::UserMenuState::from_items(dir, items));
        }
        user_menu::MenuFile::FarMenuFound(far_path) => {
            app.mode = Mode::ConfirmPortFarMenu(far_path);
        }
        user_menu::MenuFile::NotFound => {
            let Some(path) = user_menu::create_menu_file(&dir) else {
                return;
            };
            let syntax_theme = app.syntax_theme.clone();
            if let Ok(editor) = Editor::open(path, syntax_theme) {
                app.mode = Mode::Editing(editor);
            }
        }
    }
}


/// Whether the entry under the cursor is a directory (`..` included) —
/// the branch point both `EnterSelected` (plain `Enter`) and
/// `OpenInFileManager` (`Shift+Enter`) need: a file always opens in the
/// built-in editor regardless of which of the two was pressed
/// (requested directly — plain `Enter` used to be a no-op on a file,
/// only `F4` opened it), and only a directory tells the two apart (one
/// navigates the panel into it, the other hands it to the OS file
/// manager). An empty panel (no entry at all) reads as "not a
/// directory", same as `false` — both callers fall through to
/// `open_editor`, which itself no-ops on a `None` `selected_path()`.
fn current_entry_is_dir(app: &mut App) -> bool {
    app.active_panel().current().is_some_and(|entry| entry.is_dir)
}


/// Opens the file under the cursor in the built-in editor (`editor.rs`,
/// backed by `edtui`). Does nothing for directories (never actually
/// reached for one — see `current_entry_is_dir` above — but kept as a
/// real guard rather than an assumption), and for files that fail to
/// load as UTF-8 text (binary files aren't supported yet — see
/// `TODO/editor.md`) rather than crashing the app.
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


/// `Shift+Enter` on a directory: hands it off to the OS's own file
/// manager (`system_open::open`) instead of navigating the panel into
/// it. `".."` isn't a real, separately-openable entry the way an
/// ordinary subdirectory is -- it's a navigation aid pointing at the
/// panel's *parent*, but the directory actually being browsed right now
/// (and the one this command should reveal) is the panel's own current
/// `path`. A first attempt resolved `".."` to that parent directory
/// instead (mirroring `Panel::enter_selected`'s own handling) -- fixed
/// once retested against the real report: with the cursor on `..`,
/// `Shift+Enter` should open the panel's own current directory, not
/// jump a level further up. A spawn failure (the OS command itself
/// missing, e.g. `xdg-open` on a minimal Linux install) is logged, not
/// surfaced to the user as an app error -- same reasoning as every
/// other external process spawn in this codebase.
fn open_directory_in_file_manager(app: &mut App) {
    let Some(path) = directory_open_target(app.active_panel()) else {
        return;
    };

    if let Err(err) = system_open::open(&path) {
        warn!(path = %path.display(), %err, "failed to open directory in the OS file manager");
    }
}


/// The real filesystem path `open_directory_in_file_manager` should
/// hand to the OS -- split out from it so this (the `..` special case,
/// see that function's own doc comment) is testable without spawning a
/// real process. `None` only for an empty panel (no entry under the
/// cursor at all).
fn directory_open_target(panel: &Panel) -> Option<PathBuf> {
    let entry = panel.current()?;
    if entry.name == ".." {
        Some(panel.path.clone())
    } else {
        Some(panel.path.join(&entry.name))
    }
}


/// F8: opens the "delete this?" prompt (`Mode::ConfirmDelete`) for
/// every entry marked in the active panel if any are, otherwise the
/// entry under the cursor (`Panel::marked_or_current`), same "marked
/// set wins" rule F5/F6 use. Does nothing if there's nothing to act on
/// (the cursor on `..` and nothing marked, or an empty panel) — never
/// deletes directly, see `confirm::handle_confirm_delete_key` for the
/// actual filesystem call.
fn request_delete(app: &mut App) {
    let panel = app.active_panel();
    let entries: Vec<DeleteEntry> = panel
        .marked_or_current()
        .into_iter()
        .map(|entry| DeleteEntry { path: panel.path.join(&entry.name), name: entry.name.clone(), is_dir: entry.is_dir, size: entry.size })
        .collect();
    if entries.is_empty() {
        return;
    }

    app.mode = Mode::ConfirmDelete(PendingDelete { entries });
}


/// F5/F6: opens the "copy/move to?" prompt (`Mode::ConfirmTransfer`)
/// for every entry `transfer_sources` picks (the marked set if
/// anything's marked, the cursor entry otherwise), pre-filled with the
/// *other* panel's directory as the destination — Far Manager's own
/// F5/F6 default. Does nothing if there's nothing to act on (an empty
/// panel with the cursor on `..` and nothing marked), same as
/// `request_delete`.
fn request_transfer(app: &mut App, operation: TransferOp) {
    let destination_dir = app.panels[1 - app.active].path.clone();

    let sources = transfer_sources(app.active_panel());
    if sources.is_empty() {
        return;
    }

    // A single entry keeps the existing behavior exactly: the
    // destination is the *full* target path (other panel's directory +
    // this entry's own name), editable in place -- including renaming
    // it during the transfer. Several entries have no single path that
    // could do that for all of them at once, so the destination
    // defaults to just the target *directory* instead -- each source's
    // own name gets joined onto it individually at transfer time
    // (`confirm::run_confirmed_transfer`).
    let destination = if let [only] = sources.as_slice() {
        destination_dir.join(&only.name).to_string_lossy().into_owned()
    } else {
        destination_dir.to_string_lossy().into_owned()
    };
    let cursor = destination.chars().count();
    let pending = PendingTransfer {
        operation,
        sources,
        destination,
        cursor,
        selection_anchor: None,
    };
    app.mode = Mode::ConfirmTransfer(pending);
}


/// The entries F5/F6 should act on — see `Panel::marked_or_current`'s
/// own doc comment for the "marked set wins over the cursor" rule this
/// (and `request_delete`, for F8) shares.
fn transfer_sources(panel: &Panel) -> Vec<TransferSource> {
    panel.marked_or_current().into_iter().map(|entry| TransferSource { path: panel.path.join(&entry.name), name: entry.name.clone(), is_dir: entry.is_dir }).collect()
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
        sources: vec![TransferSource { path, name: entry.name.clone(), is_dir: entry.is_dir }],
        destination,
        cursor,
        selection_anchor: None,
    };
    app.mode = Mode::ConfirmTransfer(pending);
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    fn scratch_dir() -> std::path::PathBuf {
        unique_scratch_dir("command")
    }

    /// Both panels rooted at a real scratch directory containing one
    /// real file `name`, with the active panel's cursor already on it
    /// (never on the synthetic `..` entry).
    fn app_with_selected_file(name: &str) -> App {
        let dir = scratch_dir();
        fs::write(dir.join(name), b"data").expect("write scratch file");
        let mut app = test_app(dir);
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
        assert_eq!(pending.entries.len(), 1);
        assert_eq!(pending.entries[0].path, expected_path);
        assert_eq!(pending.entries[0].name, "victim.txt");
        assert!(!pending.entries[0].is_dir);
    }

    /// Regression coverage for the real request: F8 should act on every
    /// marked entry, not just the one under the cursor.
    #[test]
    fn request_delete_uses_the_marked_set_instead_of_the_cursor_once_anything_is_marked() {
        let dir = scratch_dir();
        fs::write(dir.join("a.txt"), b"a").unwrap();
        fs::write(dir.join("b.txt"), b"b").unwrap();
        fs::write(dir.join("c.txt"), b"c").unwrap();
        let mut app = test_app(dir);
        let panel = &mut app.panels[0];
        let c_index = panel.entries.iter().position(|e| e.name == "c.txt").unwrap();
        panel.selected = panel.entries.iter().position(|e| e.name == "a.txt").unwrap();
        panel.toggle_mark_move_down();
        panel.selected = panel.entries.iter().position(|e| e.name == "b.txt").unwrap();
        panel.toggle_mark_move_down();
        panel.selected = c_index; // cursor ends up on the unmarked entry

        request_delete(&mut app);

        let Mode::ConfirmDelete(pending) = &app.mode else {
            panic!("expected Mode::ConfirmDelete");
        };
        let mut names: Vec<&str> = pending.entries.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["a.txt", "b.txt"], "the marked entries, not the cursor entry (c.txt)");
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
        let mut app = test_app(scratch_dir());

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
        assert_eq!(pending.sources.len(), 1);
        assert_eq!(pending.sources[0].name, "source.txt");
    }

    /// Regression coverage for the real request: F5/F6 should act on
    /// every marked entry, not just the one under the cursor -- and the
    /// marked set wins even when the cursor sits on a third, unmarked
    /// entry.
    #[test]
    fn request_transfer_uses_the_marked_set_instead_of_the_cursor_once_anything_is_marked() {
        let dir = scratch_dir();
        fs::write(dir.join("a.txt"), b"a").unwrap();
        fs::write(dir.join("b.txt"), b"b").unwrap();
        fs::write(dir.join("c.txt"), b"c").unwrap();
        let mut app = test_app(dir);
        let other_dir = app.panels[1].path.clone();
        let panel = &mut app.panels[0];
        let c_index = panel.entries.iter().position(|e| e.name == "c.txt").unwrap();
        panel.selected = panel.entries.iter().position(|e| e.name == "a.txt").unwrap();
        panel.toggle_mark_move_down(); // marks a.txt and moves off it
        panel.selected = panel.entries.iter().position(|e| e.name == "b.txt").unwrap();
        panel.toggle_mark_move_down(); // marks b.txt too
        panel.selected = c_index; // cursor ends up on the unmarked entry

        request_transfer(&mut app, TransferOp::Copy);

        let Mode::ConfirmTransfer(pending) = &app.mode else {
            panic!("expected Mode::ConfirmTransfer");
        };
        let mut names: Vec<&str> = pending.sources.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["a.txt", "b.txt"], "the marked entries, not the cursor entry (c.txt)");
        assert_eq!(pending.destination, other_dir.to_string_lossy(), "several sources default to just the target directory");
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

    mod current_entry_is_dir_tests {
        use super::*;

        #[test]
        fn true_for_a_directory_entry() {
            let dir = scratch_dir();
            fs::create_dir(dir.join("sub")).unwrap();
            let mut app = test_app(dir);
            app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "sub").unwrap();

            assert!(current_entry_is_dir(&mut app));
        }

        #[test]
        fn true_for_dotdot() {
            let mut app = app_with_selected_file("victim.txt");
            app.panels[0].selected = 0; // ".." is always entry 0 when a parent exists

            assert!(current_entry_is_dir(&mut app));
        }

        #[test]
        fn false_for_a_file_entry() {
            let mut app = app_with_selected_file("victim.txt");

            assert!(!current_entry_is_dir(&mut app));
        }

        /// A panel rooted at a non-root scratch directory always lists
        /// at least `..` (see `panel::tests::panel_with_dotdot_and`'s
        /// own reasoning) -- genuinely empty (`current()` returning
        /// `None`) only happens with no entries at all, which needs
        /// clearing `entries` directly rather than trusting an ordinary
        /// scratch directory to produce it.
        #[test]
        fn false_for_an_empty_panel() {
            let mut app = test_app(scratch_dir());
            app.panels[0].entries.clear();

            assert!(!current_entry_is_dir(&mut app));
        }
    }

    mod enter_selected_and_open_in_file_manager_tests {
        use super::*;

        /// Regression coverage for the real request: plain `Enter` on a
        /// file used to be a no-op (only `F4`/`EditSelected` opened the
        /// editor) -- it should now open the built-in editor, exactly
        /// like `F4`.
        #[test]
        fn enter_selected_on_a_file_opens_the_editor() {
            let mut app = app_with_selected_file("victim.txt");

            execute(Command::EnterSelected, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)));
        }

        /// `Shift+Enter` on a file behaves exactly like plain `Enter` --
        /// requested directly, so the two only differ on a directory.
        #[test]
        fn open_in_file_manager_on_a_file_also_opens_the_editor() {
            let mut app = app_with_selected_file("victim.txt");

            execute(Command::OpenInFileManager, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)));
        }

        /// Plain `Enter` on a directory still navigates the panel into
        /// it -- unaffected by the file-opens-the-editor change above.
        #[test]
        fn enter_selected_on_a_directory_still_navigates_into_it() {
            let dir = scratch_dir();
            fs::create_dir(dir.join("sub")).unwrap();
            let mut app = test_app(dir.clone());
            app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "sub").unwrap();

            execute(Command::EnterSelected, &mut app).unwrap();

            assert_eq!(app.panels[0].path, dir.join("sub"));
            assert!(matches!(app.mode, Mode::Browsing));
        }
    }

    mod directory_open_target_tests {
        use super::*;

        #[test]
        fn targets_the_subdirectory_under_the_cursor() {
            let dir = scratch_dir();
            fs::create_dir(dir.join("sub")).unwrap();
            let mut app = test_app(dir.clone());
            app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "sub").unwrap();

            let target = directory_open_target(&app.panels[0]);

            assert_eq!(target, Some(dir.join("sub")));
        }

        /// Regression coverage for the real report, in two rounds: a
        /// first attempt resolved `..` to the panel's *parent*
        /// (`panel.path.parent()`) -- reported wrong on retest: with the
        /// cursor on `..`, `Shift+Enter` should open the panel's own
        /// *current* directory (what's actually being browsed right
        /// now), not jump a level further up. `..` isn't a distinct
        /// entry with somewhere else of its own to point Explorer at.
        #[test]
        fn targets_the_panels_own_current_directory_for_dotdot_not_its_parent() {
            let dir = scratch_dir();
            let sub = dir.join("sub");
            fs::create_dir(&sub).unwrap();
            let mut app = test_app(sub.clone());
            app.panels[0].selected = 0; // ".." is always entry 0 when a parent exists

            let target = directory_open_target(&app.panels[0]);

            assert_eq!(target, Some(sub), "should resolve to the panel's own current directory, not its parent");
        }

        /// Same "genuinely empty" caveat as `current_entry_is_dir_tests
        /// ::false_for_an_empty_panel` -- a non-root scratch directory
        /// always has `..`, so `entries` needs clearing directly rather
        /// than relying on a fresh scratch directory to be entry-less.
        #[test]
        fn none_for_an_empty_panel() {
            let mut app = test_app(scratch_dir());
            app.panels[0].entries.clear();

            assert_eq!(directory_open_target(&app.panels[0]), None);
        }
    }

    mod open_user_menu_tests {
        use super::*;

        #[test]
        fn opens_the_menu_when_a_litastum_menu_file_exists() {
            let dir = scratch_dir();
            fs::write(dir.join("LitastumMenu.toml"), "[[item]]\ntitle = \"status\"\nhotkey = \"s\"\ncommands = [\"git status -s\"]\n").unwrap();
            let mut app = test_app(dir);

            execute(Command::OpenUserMenu, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::UserMenu(_)));
        }

        /// Regression coverage for the real report: this used to be a
        /// silent no-op with no menu file, indistinguishable from `F2`
        /// simply not being bound at all. There's nothing to *browse*
        /// yet without a file, so it creates an empty `LitastumMenu.toml`
        /// and opens the built-in editor on it instead of an empty
        /// popup with nothing to select.
        #[test]
        fn creates_and_opens_a_new_menu_file_when_neither_exists() {
            let dir = scratch_dir();
            let mut app = test_app(dir.clone());

            execute(Command::OpenUserMenu, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)), "should open the built-in editor on the freshly created file");
            assert!(dir.join("LitastumMenu.toml").is_file());
        }

        /// A real `FarMenu.ini` should be *offered*, not read or
        /// converted directly -- the actual point of the port-on-
        /// confirm flow requested directly, instead of the earlier
        /// silent-migration behavior.
        #[test]
        fn offers_to_port_a_far_menu_instead_of_reading_it_directly() {
            let dir = scratch_dir();
            fs::write(dir.join("FarMenu.ini"), "s: status\ngit status -s\n").unwrap();
            let mut app = test_app(dir.clone());

            execute(Command::OpenUserMenu, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::ConfirmPortFarMenu(_)));
            assert!(!dir.join("LitastumMenu.toml").exists(), "must not convert until confirmed");
        }
    }
}
