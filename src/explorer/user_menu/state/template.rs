use std::fs;
use std::path::{Path, PathBuf};

use super::OWN_FILE_NAME;

/// The commented-out-example content `create_menu_file` writes --
/// pulled out to a module-level constant (rather than local to that
/// function) so `resolve::resolve_menu_with_fallback`'s own tests can
/// write the exact same "empty template" content a real freshly-created
/// file would have, without duplicating it out of sync. `pub(super)`
/// (visible to `state` and every module nested inside it, `resolve`'s
/// own test module included) rather than `pub` -- nothing outside this
/// directory needs the literal template text.
pub(super) const EMPTY_MENU_TEMPLATE: &str = "\
# LitastumMenu.toml -- F2 user menu. Uncomment and edit:
#
# [[item]]
# title = \"status\"
# hotkey = \"s\"
# commands = [\"git status -s\"]
#
# [[item]]
# title = \"submenu example\"
# [[item.submenu]]
# title = \"nested item\"
# commands = [\"echo hi\"]
";

/// Creates a fresh `LitastumMenu.toml` in `dir`, with a commented-out
/// example to get started -- `F2` calls this when `resolve_menu` finds
/// neither file at all, so there's actually something to open in the
/// built-in editor right away (`explorer::command::open_user_menu`)
/// instead of an empty popup with nothing in it to select. `None` if
/// either step fails (a read-only directory, permissions, ...) -- `F2`
/// just does nothing then, same as any other "couldn't act on this"
/// case in this codebase.
///
/// `create_dir_all`s `dir` first, unlike the very first version of
/// this function -- needed once `open_user_menu` started passing the
/// *common config* directory here instead of the always-already-real
/// active panel directory: the OS config directory (`%APPDATA%\litastum\`
/// or equivalent) may not exist yet at all on a machine where no
/// theme/setup has ever been saved, and a plain `fs::write` fails
/// outright when its parent directory is missing. A no-op for the
/// already-real active-directory case this function still also serves
/// (`create_menu_file_tests`, `UserMenuState`'s own persistence).
pub fn create_menu_file(dir: &Path) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let path = dir.join(OWN_FILE_NAME);
    fs::write(&path, EMPTY_MENU_TEMPLATE).ok()?;
    Some(path)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::user_menu::state::scratch_dir;
    use crate::explorer::user_menu::state::{resolve_menu, MenuFile};

    #[test]
    fn creates_a_litastum_menu_file() {
        let dir = scratch_dir();

        let path = create_menu_file(&dir).unwrap();

        assert_eq!(path, dir.join(OWN_FILE_NAME));
        assert!(path.is_file());
    }

    #[test]
    fn the_created_file_is_then_found_by_resolve_menu() {
        let dir = scratch_dir();
        create_menu_file(&dir).unwrap();

        let MenuFile::Own(_, items) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };
        assert!(items.is_empty(), "the template is all comments, so no real items yet");
    }

    /// Regression coverage for `open_user_menu` now targeting the
    /// common config directory on a fresh `F2` instead of the
    /// active one: that directory (`%APPDATA%\litastum\` or
    /// equivalent) may not exist yet at all on a machine that has
    /// never saved a theme/setup -- a plain `fs::write` would fail
    /// outright with its parent missing, so `create_menu_file` now
    /// `create_dir_all`s first.
    #[test]
    fn creates_the_directory_itself_if_it_does_not_exist_yet() {
        let dir = scratch_dir().join("not-created-yet");
        assert!(!dir.exists());

        let path = create_menu_file(&dir).unwrap();

        assert!(dir.is_dir());
        assert!(path.is_file());
    }
}
