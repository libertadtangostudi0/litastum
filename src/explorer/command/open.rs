use std::path::PathBuf;

use tracing::warn;

use crate::app::{App, Mode, Overlay};
use crate::editor::Editor;
use crate::explorer::{image_preview, markdown_preview, system_open, user_menu, Panel};

/// `F3`: an image preview, or for a `.md` file the editor with a live
/// preview beside it; a no-op otherwise. Checked by extension here so
/// only one kind is attempted.
pub(super) fn preview_selected(app: &mut App) {
    let Some(path) = app.active_panel().selected_path() else {
        return;
    };

    if image_preview::is_supported_image(&path) {
        image_preview::open_preview(app);
    } else if markdown_preview::is_markdown_file(&path) {
        markdown_preview::open_edit_preview(app);
    }
}


/// `F2`: opens the user menu, per `user_menu::resolve_menu`:
/// - `FarMenu.ini` found -- ask before porting it
///   (`Overlay::ConfirmPortFarMenu`); wins over a `LitastumMenu.toml`.
/// - A `LitastumMenu.toml` in the active directory, else in the common
///   config directory -- browse it; edits persist to where it was found.
/// - Neither -- create an empty one in the common directory only (never
///   the active one) and open it in the editor. A failed creation is a
///   silent no-op.
///
/// History: docs/history/file-commands.md.
pub(super) fn open_user_menu(app: &mut App) {
    let dir = app.active_panel().path.clone();
    match user_menu::resolve_menu(&dir) {
        user_menu::MenuFile::Own(menu_dir, items) => {
            app.overlay = Some(Overlay::UserMenu(user_menu::UserMenuState::from_items(menu_dir, items)));
        }
        user_menu::MenuFile::FarMenuFound(far_path) => {
            app.overlay = Some(Overlay::ConfirmPortFarMenu(far_path));
        }
        user_menu::MenuFile::NotFound => {
            let Some(common_dir) = user_menu::common_menu_dir() else {
                return;
            };
            let Some(path) = user_menu::create_menu_file(&common_dir) else {
                return;
            };
            let syntax_theme = app.syntax_theme.clone();
            if let Ok(editor) = Editor::open(path, syntax_theme, app.settings.editor_keymap_mode) {
                app.mode = Mode::Editing(editor);
            }
        }
    }
}


/// Whether the entry under the cursor is a directory (`..` included).
/// `Enter` and `Shift+Enter` both open a file in the editor and only
/// differ on a directory. An empty panel reads as `false`.
pub(super) fn current_entry_is_dir(app: &mut App) -> bool {
    app.active_panel().current().is_some_and(|entry| entry.is_dir)
}


/// Opens the file under the cursor in the built-in editor. No-op for a
/// directory, or a file that doesn't load as UTF-8 (`TODO/editor.md`).
pub(super) fn open_editor(app: &mut App) {
    let Some(path) = app.active_panel().selected_path() else {
        return;
    };
    if path.is_dir() {
        return;
    }

    let syntax_theme = app.syntax_theme.clone();
    if let Ok(editor) = Editor::open(path, syntax_theme, app.settings.editor_keymap_mode) {
        app.mode = Mode::Editing(editor);
    }
}


/// `Shift+Enter` on a directory: opens it in the OS file manager. A
/// spawn failure is logged, not shown.
pub(super) fn open_directory_in_file_manager(app: &mut App) {
    let Some(path) = directory_open_target(app.active_panel()) else {
        return;
    };

    if let Err(err) = system_open::open(&path) {
        warn!(path = %path.display(), %err, "failed to open directory in the OS file manager");
    }
}


/// The directory to open: `..` means the panel's own directory (what is
/// being browsed), not its parent. `None` for an empty panel. History: docs/history/file-commands.md.
fn directory_open_target(panel: &Panel) -> Option<PathBuf> {
    let entry = panel.current()?;
    if entry.name == ".." {
        Some(panel.path.clone())
    } else {
        Some(panel.path.join(&entry.name))
    }
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::explorer::command::{app_with_selected_file, scratch_dir};
    use crate::explorer::keymap::Command;
    use crate::test_support::test_app;

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
        use crate::explorer::command::execute;

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

        /// `..` opens the directory being browsed; resolving it to the
        /// parent was reported wrong.
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
        use crate::explorer::command::execute;

        #[test]
        fn opens_the_menu_when_a_litastum_menu_file_exists() {
            let dir = scratch_dir();
            fs::write(dir.join("LitastumMenu.toml"), "[[item]]\ntitle = \"status\"\nhotkey = \"s\"\ncommands = [\"git status -s\"]\n").unwrap();
            let mut app = test_app(dir);

            execute(Command::OpenUserMenu, &mut app).unwrap();

            assert!(matches!(app.overlay, Some(Overlay::UserMenu(_))));
        }

        /// With no menu anywhere, `F2` creates one in the common config
        /// directory, never the active one. `common_menu_dir` is `None`
        /// in tests, so this pins the no-op half; `create_menu_file`'s
        /// own tests cover creation.
        #[test]
        fn does_nothing_when_neither_exists_and_no_config_directory_is_available() {
            let dir = scratch_dir();
            let mut app = test_app(dir.clone());

            execute(Command::OpenUserMenu, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::Browsing), "should be a silent no-op, same as any other unavailable-target case");
            assert!(!dir.join("LitastumMenu.toml").exists(), "must not fall back to creating it in the active directory");
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

            assert!(matches!(app.overlay, Some(Overlay::ConfirmPortFarMenu(_))));
            assert!(!dir.join("LitastumMenu.toml").exists(), "must not convert until confirmed");
        }
    }
}
