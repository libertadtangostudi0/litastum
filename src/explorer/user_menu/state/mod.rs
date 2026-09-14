//! `state.rs` split into one file per concern once it passed the
//! project's own ~500-line decomposition threshold
//! ([[code-conventions]]) -- 1493 lines, mixing file-resolution
//! (`resolve`), one-time `FarMenu.ini` migration (`porting`), the
//! fresh-file template (`template`), the actual browsing/editing tree
//! (`browsing`), the `Ins` add-item form (`add_item`), the `F4`
//! command-edit scratch-file flow (`command_edit`), and the
//! `!?Label?Default!` prompt-collection popup (`prompt`) all in one
//! place. Every symbol that was `pub` at the top of the old file is
//! re-exported here unchanged, so nothing outside this directory (or
//! its own tests) needed to change to account for the split --
//! `explorer/user_menu.rs`'s own `pub use state::{...}` list and
//! `input.rs`'s `state::create_command_edit_file`/`state::port_far_menu`/
//! `state::backup_far_menu_without_porting` calls still resolve exactly
//! as before.
//!
//! `OWN_FILE_NAME`/`FAR_FILE_NAME` stay here, at the shared root, rather
//! than living in whichever submodule happens to use them most
//! (`resolve`) -- `porting` and `browsing` both need them too, and a
//! plain (non-`pub`) item declared here is already visible to every
//! descendant module of this one, no `pub(super)` juggling needed for
//! the common case.

mod add_item;
mod browsing;
mod command_edit;
mod porting;
mod prompt;
mod resolve;
mod template;

pub use add_item::AddUserMenuItemState;
pub use browsing::UserMenuState;
pub use command_edit::{create_command_edit_file, finish_command_edit, UserMenuCommandEdit};
pub use porting::{backup_far_menu_without_porting, port_far_menu};
pub use prompt::UserMenuPromptState;
pub use resolve::{common_menu_dir, resolve_menu, MenuFile};
pub use template::create_menu_file;

/// litastum's own user-menu file name -- a native, structured format
/// (`toml_format.rs`), not Far Manager's own hand-rolled DSL
/// (`parse.rs`). Picked over sticking with the same DSL specifically
/// so this app can read-modify-write it programmatically later
/// (adding/removing an item from the UI) without a bespoke serializer.
const OWN_FILE_NAME: &str = "LitastumMenu.toml";
/// Real Far Manager's own per-directory user-menu file name.
const FAR_FILE_NAME: &str = "FarMenu.ini";

/// Backup suffix appended to whichever file `port_far_menu`/
/// `backup_far_menu_without_porting` move out of the way -- a single
/// slot, not a timestamped one: this is a rare, explicitly-confirmed
/// action, and clobbering an *older* backup on a second port is an
/// acceptable trade-off for not reinventing unique-file-name logic
/// that already exists (differently shaped) in `find_file/export.rs`.
const BACKUP_SUFFIX: &str = ".bak";

/// Shared by every submodule's own `#[cfg(test)] mod tests` below
/// (`resolve::tests`, `porting::tests`, ...) -- a plain, non-`pub`
/// item here is already visible to every descendant module, same as
/// `OWN_FILE_NAME`/`FAR_FILE_NAME` above, so no visibility juggling is
/// needed to share this one test helper across them.
#[cfg(test)]
fn scratch_dir() -> std::path::PathBuf {
    crate::test_support::unique_scratch_dir("user-menu")
}
