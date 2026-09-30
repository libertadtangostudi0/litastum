use std::fs;
use std::path::{Path, PathBuf};

use super::OWN_FILE_NAME;

/// The commented-out example `create_menu_file` writes, shared with
/// `resolve`'s tests so they write exactly the same "empty" file.
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

/// Creates a `LitastumMenu.toml` with a commented-out example in `dir`,
/// creating `dir` first (the config directory may not exist yet). `None`
/// on failure; `F2` then does nothing.
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
