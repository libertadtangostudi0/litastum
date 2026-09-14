use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::{FAR_FILE_NAME, OWN_FILE_NAME};
use crate::explorer::user_menu::parse::MenuItem;
use crate::explorer::user_menu::toml_format;
use crate::theming::config::config_dir;

/// What `F2` (or the startup check in `main.rs`) finds in a directory --
/// drives `explorer::command::open_user_menu`'s own branching between
/// browsing, offering to port a `FarMenu.ini`, or creating a fresh
/// file.
pub enum MenuFile {
    /// `LitastumMenu.toml` exists (and no `FarMenu.ini` is sitting next
    /// to it -- see `resolve_menu`'s own doc comment for why that takes
    /// priority) -- a malformed file still resolves to an empty item
    /// list rather than an error (see `toml_format::parse_toml`'s own
    /// doc comment), so this variant covers both cases. The `PathBuf`
    /// is the *directory* it was found in -- the active panel's own
    /// directory for a local menu, or the common config directory for
    /// the fallback below -- passed straight through to
    /// `UserMenuState::from_items` so edits persist back to wherever
    /// this particular menu actually came from.
    Own(PathBuf, Vec<MenuItem>),
    /// A `FarMenu.ini` is here -- offer to port it
    /// (`Mode::ConfirmPortFarMenu`) rather than reading or converting
    /// it silently. Reported *even if* `LitastumMenu.toml` also
    /// exists already -- dropping a `FarMenu.ini` into an already-
    /// configured directory should still surface the choice (port and
    /// overwrite, backing up the old config first, or decline and just
    /// have `FarMenu.ini` backed up out of the way) rather than being
    /// silently ignored.
    FarMenuFound(PathBuf),
    /// Neither file exists anywhere `resolve_menu` looked (the active
    /// directory nor the common config directory).
    NotFound,
}

/// Looks for a user menu in `dir` -- either directly, or (if nothing is
/// there at all) in the common config directory, so a menu set up once
/// is available from any directory on any drive, not just the one it
/// was created in. Real per-directory menus still always win: the
/// common one is only consulted when `dir` itself has neither file,
/// matching real Far Manager's own local-then-common precedence for
/// its `menu.ini`. Reported directly, against a real Subversion working
/// copy far from wherever the menu had actually been set up: switching
/// to another directory/drive showed an empty menu -- `resolve_menu`
/// used to only ever look at `dir`, so any directory without its own
/// `LitastumMenu.toml` showed nothing no matter what.
///
/// `config_dir()` (`theming::config`, reused here rather than
/// duplicated -- it's what every other per-user file, `config.json`/
/// `themes/`, already resolves through) also doubles as this app's one
/// local-development escape hatch: set `LITASTUM_CONFIG_DIR` to point
/// it at the project checkout instead of the real
/// `%APPDATA%\litastum\`, so testing this fallback doesn't mean
/// creating files in the real per-user config directory by hand. See
/// `config_dir`'s own doc comment.
///
/// The actual per-directory lookup (`resolve_menu_in`) is pulled out
/// separately so `resolve_menu_with_fallback` -- and this function's
/// own tests -- can exercise the local/common precedence with two
/// plain scratch directories, without touching the real config
/// directory at all (same "injectable path, untested wrapper" split
/// `theming::config`'s own tests already use, for the same reason:
/// exercising the real path would mutate whatever `LitastumMenu.toml`
/// a developer running the test suite actually has sitting in it).
pub fn resolve_menu(dir: &Path) -> MenuFile {
    resolve_menu_with_fallback(dir, config_dir().as_deref())
}

/// The one common menu location `resolve_menu` falls back to -- exposed
/// so `explorer::command::open_user_menu` can create a fresh
/// `LitastumMenu.toml` *there* (not in whichever directory happened to
/// be active) when `F2` finds nothing anywhere, per its own doc
/// comment. `None` if the platform gives us no config directory at
/// all (see `config_dir`'s own doc comment) -- same "just don't create
/// anything" fallback `create_menu_file`'s own failure case already
/// has.
pub fn common_menu_dir() -> Option<PathBuf> {
    config_dir()
}

/// An *empty* local `LitastumMenu.toml` (parses to zero items -- either
/// genuinely blank, or just the commented-out-example template
/// `create_menu_file` writes) doesn't count as "found" for fallback
/// purposes either -- reported directly: a directory where `F2` had
/// been pressed once before this fallback existed (creating that
/// template and nothing else) permanently shadowed the common menu
/// from then on, even though there was nothing real in the local file
/// to prefer over it. A local `FarMenu.ini`, or a local
/// `LitastumMenu.toml` with at least one real item, still always wins
/// -- this only widens what counts as "nothing here yet."
fn resolve_menu_with_fallback(dir: &Path, common_dir: Option<&Path>) -> MenuFile {
    match resolve_menu_in(dir) {
        Some(MenuFile::Own(local_dir, items)) if items.is_empty() => {
            common_dir.and_then(resolve_menu_in).unwrap_or(MenuFile::Own(local_dir, items))
        }
        Some(result) => result,
        None => common_dir.and_then(resolve_menu_in).unwrap_or(MenuFile::NotFound),
    }
}

/// A `FarMenu.ini` takes priority over an already-existing
/// `LitastumMenu.toml` -- reported directly: dropping a `FarMenu.ini`
/// into a directory that already has a configured menu used to be
/// silently ignored (this function returned `Own` without even
/// checking for `FarMenu.ini`), which meant there was no way to
/// deliberately re-import one short of deleting `LitastumMenu.toml`
/// first. Never writes anything itself -- porting (`port_far_menu`) or
/// backing `FarMenu.ini` out of the way (`backup_far_menu_without_porting`)
/// only happens once the user actually answers the prompt this
/// produces (`Mode::ConfirmPortFarMenu`). `None` if `dir` has neither
/// file, letting `resolve_menu_with_fallback` try the next directory.
fn resolve_menu_in(dir: &Path) -> Option<MenuFile> {
    let far = dir.join(FAR_FILE_NAME);
    if far.is_file() {
        return Some(MenuFile::FarMenuFound(far));
    }

    let own = dir.join(OWN_FILE_NAME);
    if own.is_file() {
        return Some(match fs::read_to_string(&own) {
            Ok(content) => MenuFile::Own(dir.to_path_buf(), toml_format::parse_toml(&content)),
            Err(err) => {
                warn!(path = %own.display(), %err, "LitastumMenu.toml exists but could not be read");
                MenuFile::Own(dir.to_path_buf(), Vec::new())
            }
        });
    }

    None
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use super::super::template::EMPTY_MENU_TEMPLATE;
    use crate::explorer::user_menu::state::scratch_dir;

    mod resolve_tests {
        use super::*;

        #[test]
        fn not_found_when_neither_file_exists() {
            // `resolve_menu_with_fallback` with an explicit `None`, not
            // the public `resolve_menu` -- that goes through the real
            // config directory (`config_dir()`), which would make this
            // test's outcome depend on whatever `LitastumMenu.toml` a
            // developer running the suite actually happens to have
            // sitting there. See `resolve_menu`'s own doc comment.
            assert!(matches!(resolve_menu_with_fallback(&scratch_dir(), None), MenuFile::NotFound));
        }

        #[test]
        fn reads_an_existing_litastum_menu() {
            let dir = scratch_dir();
            fs::write(dir.join(OWN_FILE_NAME), "[[item]]\ntitle = \"status\"\nhotkey = \"s\"\ncommands = [\"git status -s\"]\n").unwrap();

            let MenuFile::Own(_, items) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };

            assert_eq!(items.len(), 1);
            assert_eq!(items[0].title, "status");
        }

        /// Regression coverage for the real request: dropping a
        /// `FarMenu.ini` into a directory that already has a configured
        /// `LitastumMenu.toml` used to be silently ignored (the old
        /// `Own`-wins precedence). It should now still surface the
        /// choice -- `FarMenuFound`, not `Own` -- so porting can
        /// deliberately overwrite (with a backup) an already-configured
        /// menu.
        #[test]
        fn a_far_menu_takes_priority_over_an_existing_litastum_menu() {
            let dir = scratch_dir();
            fs::write(dir.join(OWN_FILE_NAME), "[[item]]\ntitle = \"mine\"\ncommands = [\"echo mine\"]\n").unwrap();
            fs::write(dir.join(FAR_FILE_NAME), "s: theirs\necho theirs\n").unwrap();

            let result = resolve_menu(&dir);

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == dir.join(FAR_FILE_NAME)));
        }

        /// The whole point of the port-on-confirm flow: a real
        /// `FarMenu.ini` should be *found*, not read directly or
        /// silently converted -- `resolve_menu` only reports it, leaving the
        /// actual conversion to `port_far_menu` once confirmed.
        #[test]
        fn reports_a_far_menu_without_reading_or_converting_it() {
            let dir = scratch_dir();
            fs::write(dir.join(FAR_FILE_NAME), "s: status\ngit status -s\n").unwrap();

            let result = resolve_menu(&dir);

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == dir.join(FAR_FILE_NAME)));
            assert!(!dir.join(OWN_FILE_NAME).exists(), "resolve alone must not create LitastumMenu.toml");
        }
    }

    /// The actual point of this whole change: a menu set up once should
    /// be reachable from any directory, not just the one it was created
    /// in -- regression coverage for the real report (switching to
    /// another directory made the menu unreadable). Exercises
    /// `resolve_menu_with_fallback` directly with two plain scratch
    /// directories standing in for "active panel dir" / "common config
    /// dir", rather than the public `resolve_menu` -- see
    /// `not_found_when_neither_file_exists`'s own comment on why the
    /// real OS config directory isn't touched by these tests.
    mod common_fallback_tests {
        use super::*;

        #[test]
        fn falls_back_to_the_common_menu_when_the_active_directory_has_none() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"status\"\ncommands = [\"git status -s\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the common directory")
            };

            assert_eq!(menu_dir, common, "edits should persist back to the common directory, not the active one");
            assert_eq!(items[0].title, "status");
        }

        #[test]
        fn a_local_menu_wins_over_the_common_one() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), "[[item]]\ntitle = \"local\"\ncommands = [\"echo local\"]\n").unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the active directory")
            };

            assert_eq!(menu_dir, active);
            assert_eq!(items[0].title, "local");
        }

        #[test]
        fn a_local_far_menu_ini_still_wins_over_a_common_litastum_menu() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(FAR_FILE_NAME), "s: theirs\necho theirs\n").unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let result = resolve_menu_with_fallback(&active, Some(&common));

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == active.join(FAR_FILE_NAME)));
        }

        #[test]
        fn a_common_far_menu_ini_is_offered_too_once_the_active_directory_has_nothing() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(common.join(FAR_FILE_NAME), "s: status\ngit status -s\n").unwrap();

            let result = resolve_menu_with_fallback(&active, Some(&common));

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == common.join(FAR_FILE_NAME)));
        }

        #[test]
        fn not_found_when_neither_directory_has_anything() {
            assert!(matches!(resolve_menu_with_fallback(&scratch_dir(), Some(&scratch_dir())), MenuFile::NotFound));
        }

        /// Regression coverage for the actual real-world report: `F2`
        /// pressed once in a directory before this fallback existed
        /// left behind an empty, commented-out-only `LitastumMenu.toml`
        /// there (`create_menu_file`'s own template) -- that stray local
        /// file should not permanently block the common menu from ever
        /// being consulted for that directory.
        #[test]
        fn an_empty_local_template_falls_through_to_the_common_menu() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), EMPTY_MENU_TEMPLATE).unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the common directory")
            };

            assert_eq!(menu_dir, common);
            assert_eq!(items[0].title, "common");
        }

        /// The flip side: with no common menu (or none configured) to
        /// fall through to, the empty local file is still what gets
        /// shown -- not `NotFound`, which would make `open_user_menu`
        /// silently overwrite it via `create_menu_file` on every `F2`.
        #[test]
        fn an_empty_local_template_is_still_shown_when_there_is_nothing_to_fall_back_to() {
            let active = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), EMPTY_MENU_TEMPLATE).unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, None) else {
                panic!("expected MenuFile::Own from the active directory")
            };

            assert_eq!(menu_dir, active);
            assert!(items.is_empty());
        }

        /// A local menu with at least one real item still wins over the
        /// common one -- only a genuinely *empty* local file falls
        /// through, per the two tests above.
        #[test]
        fn a_non_empty_local_menu_still_wins_over_the_common_one() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), "[[item]]\ntitle = \"local\"\ncommands = [\"echo local\"]\n").unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the active directory")
            };

            assert_eq!(menu_dir, active);
            assert_eq!(items[0].title, "local");
        }
    }
}
