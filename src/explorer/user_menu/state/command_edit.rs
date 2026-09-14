use std::fs;
use std::io;
use std::path::PathBuf;

use tracing::warn;

use super::UserMenuState;

/// `F4` on a highlighted `Commands` item: editing just that item's own
/// command(s) in the real built-in editor, not a bespoke single-line UI
/// form and not the whole `LitastumMenu.toml` file. Reported directly,
/// twice: a first attempt opened the whole file
/// (`explorer::user_menu::input::edit_menu_file`, since removed), a
/// second opened a small in-popup text field
/// (`EditUserMenuItemState`, since removed) -- both missed the actual
/// ask (just this item's command(s), but in the real editor, with its
/// own undo/syntax highlighting/multi-line editing, not a one-line
/// form).
///
/// `temp_path` is a scratch file *outside* the project, holding just
/// this item's commands, one per line -- opened in `Mode::Editing` like
/// any other file. Held alongside `menu` (the same "park the state,
/// hand it back once the editor really closes" shape
/// `AddUserMenuItem`/`ConfirmDiscard` already use) so
/// `editor_keymap::return_from_editor` can finish the edit
/// (`finish_command_edit`) once the editor session actually ends.
pub struct UserMenuCommandEdit {
    pub menu: UserMenuState,
    pub temp_path: PathBuf,
}

/// Writes `commands` (the selected item's own command lines) to a fresh
/// scratch file, one line per command, so `F4` can open it in the real
/// built-in editor instead of a bespoke UI form. The file lives in the
/// OS temp directory, not the project -- it's a working copy for this
/// one edit, never itself part of `LitastumMenu.toml`. A monotonic
/// counter (alongside the process id) keeps concurrent edits (or, more
/// realistically, concurrent tests) from colliding on the same path.
pub fn create_command_edit_file(commands: &[String]) -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("litastum-menu-command-{}-{n}.sh", std::process::id()));
    fs::write(&path, commands.join("\n"))?;
    Ok(path)
}

/// Finishes an `F4` command-edit session, once the real built-in editor
/// has genuinely closed (`editor_keymap::return_from_editor`): reads
/// back whatever `edit.temp_path` now contains (each non-blank line
/// becomes one command), replaces the selected item's commands with it
/// (`UserMenuState::replace_selected_commands`, which also persists),
/// and deletes the scratch file. Reading rather than trusting an
/// in-memory copy means this works the same whether the user actually
/// saved (`Ctrl+S`) or the "unsaved changes, discard?" prompt discarded
/// them -- either way the file on disk is the source of truth, same as
/// opening any other file in this editor. A file that couldn't be read
/// (deleted out from under us, or never wrote successfully) just leaves
/// the item's commands untouched, same "never block on a failed
/// read/write" rule the rest of this module follows.
pub fn finish_command_edit(edit: UserMenuCommandEdit) -> UserMenuState {
    let commands = fs::read_to_string(&edit.temp_path)
        .map(|content| content.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Err(err) = fs::remove_file(&edit.temp_path) {
        warn!(path = %edit.temp_path.display(), %err, "failed to remove the F4 command-edit scratch file");
    }

    let mut menu = edit.menu;
    menu.replace_selected_commands(commands);
    menu
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::user_menu::parse::{self, MenuItemBody};
    use crate::explorer::user_menu::state::scratch_dir;
    use crate::explorer::user_menu::state::{resolve_menu, MenuFile};

    #[test]
    fn create_command_edit_file_writes_one_command_per_line() {
        let path = create_command_edit_file(&["git status -s".to_string(), "echo done".to_string()]).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "git status -s\necho done");
        fs::remove_file(&path).unwrap();
    }

    /// The actual point of the whole feature: `F4` opens a real
    /// editor session on a scratch file, and closing it feeds
    /// whatever's now in that file back into the item -- title
    /// untouched, persisted, scratch file cleaned up.
    #[test]
    fn finish_command_edit_applies_the_files_current_contents_and_deletes_it() {
        let dir = scratch_dir();
        let menu = UserMenuState::from_items(dir.clone(), parse::parse(": status\necho hi\n"));
        let temp_path = create_command_edit_file(&["echo hi".to_string()]).unwrap();
        fs::write(&temp_path, "git status -sb\necho done\n").unwrap(); // simulates an edit + save

        let menu = finish_command_edit(UserMenuCommandEdit { menu, temp_path: temp_path.clone() });

        assert_eq!(menu.current_level().items[0].title, "status", "title should be untouched");
        assert_eq!(menu.current_level().items[0].body, MenuItemBody::Commands(vec!["git status -sb".to_string(), "echo done".to_string()]));
        assert!(!temp_path.exists(), "the scratch file should have been cleaned up");
        let MenuFile::Own(_, reread) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };
        assert_eq!(reread[0].body, MenuItemBody::Commands(vec!["git status -sb".to_string(), "echo done".to_string()]));
    }

    /// An unmodified (or emptied-and-discarded) scratch file just
    /// re-applies the same commands it started with -- covers the
    /// "closed without saving" path, where the file on disk never
    /// changed from what `create_command_edit_file` wrote.
    #[test]
    fn finish_command_edit_with_an_unmodified_file_leaves_commands_unchanged() {
        let dir = scratch_dir();
        let menu = UserMenuState::from_items(dir.clone(), parse::parse(": status\necho hi\n"));
        let temp_path = create_command_edit_file(&["echo hi".to_string()]).unwrap();

        let menu = finish_command_edit(UserMenuCommandEdit { menu, temp_path });

        assert_eq!(menu.current_level().items[0].body, MenuItemBody::Commands(vec!["echo hi".to_string()]));
    }
}
