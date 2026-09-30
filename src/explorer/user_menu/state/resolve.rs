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
    /// `LitastumMenu.toml` found (and no `FarMenu.ini` beside it); a malformed
    /// file gives an empty list, not an error. The `PathBuf` is the directory
    /// it was found in -- the panel's or the common one -- where edits persist.
    Own(PathBuf, Vec<MenuItem>),
    /// A `FarMenu.ini` is here: offer to port it, even if `LitastumMenu.toml`
    /// exists too.
    FarMenuFound(PathBuf),
    /// Neither file exists anywhere `resolve_menu` looked (the active
    /// directory nor the common config directory).
    NotFound,
}

/// Finds the menu for `dir`: `dir` itself first, else the common config
/// directory, so one menu works from anywhere (Far's local-then-common
/// precedence). The lookup is split out
/// (`resolve_menu_with_fallback`) so tests use scratch directories, never
/// the real config directory. History: docs/history/user-menu.md.
pub fn resolve_menu(dir: &Path) -> MenuFile {
    resolve_menu_with_fallback(dir, config_dir().as_deref())
}

/// The common menu directory, where `F2` creates a fresh menu when none
/// exists anywhere. `None` without a config directory.
pub fn common_menu_dir() -> Option<PathBuf> {
    config_dir()
}

/// A local `LitastumMenu.toml` with no items (blank, or just the template)
/// doesn't shadow the common menu. History: docs/history/user-menu.md.
fn resolve_menu_with_fallback(dir: &Path, common_dir: Option<&Path>) -> MenuFile {
    match resolve_menu_in(dir) {
        Some(MenuFile::Own(local_dir, items)) if items.is_empty() => {
            common_dir.and_then(resolve_menu_in).unwrap_or(MenuFile::Own(local_dir, items))
        }
        Some(result) => result,
        None => common_dir.and_then(resolve_menu_in).unwrap_or(MenuFile::NotFound),
    }
}

/// One directory's menu: `FarMenu.ini` wins over `LitastumMenu.toml`, so
/// re-importing one is possible. Never writes. `None` if neither exists.
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

    /// A menu set up once is reachable from other directories. Uses
    /// `resolve_menu_with_fallback` with scratch directories, never the real
    /// config directory.
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
