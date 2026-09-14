use std::path::PathBuf;

use tracing::warn;

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::explorer::{image_preview, markdown_preview, system_open, user_menu, Panel};

/// `F3`: previews the entry under the cursor -- an image
/// (`image_preview::open_preview`) or, for a `.md`/`.markdown` file, the
/// built-in editor and a live rendered preview side by side
/// (`markdown_preview::open_edit_preview`) -- checked by extension
/// before either module even tries to open it, so exactly one preview
/// kind ever runs per press. A no-op for anything else (a directory, an
/// unsupported/undecodable file) -- each module's own open function
/// already no-ops on its own non-matching cases, but checking here
/// first avoids a wasted `fs::read_dir`/decode attempt for a file
/// that's obviously the other kind.
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


/// `F2`: opens the active panel's own user menu. Three cases, per
/// `user_menu::resolve_menu`:
/// - A `FarMenu.ini` exists -- ask before touching anything
///   (`Mode::ConfirmPortFarMenu`), rather than converting or reading it
///   silently; `handle_confirm_port_far_menu_key` does the actual port
///   once confirmed. Takes priority even over an already-existing
///   `LitastumMenu.toml` (`resolve_menu`'s own doc comment) -- the same
///   startup check runs once in `main.rs`, so this same prompt can also
///   appear before `F2` is ever pressed.
/// - Only `LitastumMenu.toml` exists -- browse it directly. If the
///   active directory has neither file, `resolve_menu` falls back to a
///   *common* menu in the OS config directory before giving up, so a
///   menu set up once is available from any directory on any drive --
///   `menu_dir` (where edits persist back to) is whichever of the two
///   directories `resolve_menu` actually found something in, not
///   necessarily the active panel's own `dir`.
/// - Neither exists anywhere -- creates an empty `LitastumMenu.toml` in
///   the *common* config directory (`user_menu::common_menu_dir`), not
///   the active one, and opens it in the built-in editor instead of
///   browsing an empty popup with nothing in it to select. Deliberately
///   not the active directory: an earlier version created it there,
///   which meant every directory a fresh `F2` was ever pressed in ended
///   up with its own empty, commented-out-only `LitastumMenu.toml`
///   scattered around -- reported directly, after the common-menu
///   fallback above was added, that the template shouldn't be created
///   anywhere except the one designated (common) location. A failed
///   creation (no config directory available on this platform,
///   read-only, permissions, ...) is a silent no-op, same as any other
///   "couldn't act on this" case in this codebase.
pub(super) fn open_user_menu(app: &mut App) {
    let dir = app.active_panel().path.clone();
    match user_menu::resolve_menu(&dir) {
        user_menu::MenuFile::Own(menu_dir, items) => {
            app.mode = Mode::UserMenu(user_menu::UserMenuState::from_items(menu_dir, items));
        }
        user_menu::MenuFile::FarMenuFound(far_path) => {
            app.mode = Mode::ConfirmPortFarMenu(far_path);
        }
        user_menu::MenuFile::NotFound => {
            let Some(common_dir) = user_menu::common_menu_dir() else {
                return;
            };
            let Some(path) = user_menu::create_menu_file(&common_dir) else {
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
pub(super) fn current_entry_is_dir(app: &mut App) -> bool {
    app.active_panel().current().is_some_and(|entry| entry.is_dir)
}


/// Opens the file under the cursor in the built-in editor (`editor.rs`,
/// backed by `edtui`). Does nothing for directories (never actually
/// reached for one — see `current_entry_is_dir` above — but kept as a
/// real guard rather than an assumption), and for files that fail to
/// load as UTF-8 text (binary files aren't supported yet — see
/// `TODO/editor.md`) rather than crashing the app.
pub(super) fn open_editor(app: &mut App) {
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
pub(super) fn open_directory_in_file_manager(app: &mut App) {
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
        use crate::explorer::command::execute;

        #[test]
        fn opens_the_menu_when_a_litastum_menu_file_exists() {
            let dir = scratch_dir();
            fs::write(dir.join("LitastumMenu.toml"), "[[item]]\ntitle = \"status\"\nhotkey = \"s\"\ncommands = [\"git status -s\"]\n").unwrap();
            let mut app = test_app(dir);

            execute(Command::OpenUserMenu, &mut app).unwrap();

            assert!(matches!(app.mode, Mode::UserMenu(_)));
        }

        /// Regression coverage for the real report this originally
        /// fixed: with no menu file anywhere, `F2` used to be a silent
        /// no-op, indistinguishable from not being bound at all --
        /// fixed by creating an empty `LitastumMenu.toml` and opening
        /// the built-in editor on it right away instead of an empty
        /// popup with nothing to select.
        ///
        /// **Where** that fresh file gets created changed later, per a
        /// second real report: an earlier version created it in the
        /// *active* directory, which meant every directory `F2` was
        /// ever pressed in with nothing configured yet ended up with
        /// its own scattered, commented-out-only `LitastumMenu.toml` --
        /// `open_user_menu` now creates it in the *common* config
        /// directory (`user_menu::common_menu_dir`) instead, so a fresh
        /// menu is set up in exactly one place. `common_menu_dir` is
        /// `None` in a test build (`config_dir`'s own doc comment --
        /// tests never touch the real OS config directory or
        /// `LITASTUM_CONFIG_DIR`), so this specific integration test
        /// can only pin down the "no config directory available"
        /// half of that branch (a silent no-op, same as any other
        /// "couldn't act on this" case) -- `create_menu_file`'s own
        /// unit tests (`state.rs`) cover the actual file-creation
        /// behavior directly, with an injected path.
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

            assert!(matches!(app.mode, Mode::ConfirmPortFarMenu(_)));
            assert!(!dir.join("LitastumMenu.toml").exists(), "must not convert until confirmed");
        }
    }
}
