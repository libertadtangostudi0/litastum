//! `F2` — Far Manager's own "user menu": a per-directory list of
//! shortcuts to shell commands, defined in litastum's own
//! `LitastumMenu.toml` (structured, serde-backed) or ported from a
//! compatible `FarMenu.ini` on explicit confirmation
//! (`Mode::ConfirmPortFarMenu`) if that's all a directory has. Split
//! into `parse`/`toml_format`/`state`/`input` by concern, same shape
//! as `find_file.rs`:
//! - `parse`: `FarMenu.ini`'s own nested-block DSL, plus the `!&`/
//!   `!?Label?Default!` macro substitution — pure, no I/O. Only used to
//!   *read* a `FarMenu.ini` for one-time porting; never litastum's own
//!   file.
//! - `toml_format`: litastum's own native `LitastumMenu.toml` shape
//!   (serde `Serialize`/`Deserialize`) and the conversion to/from
//!   `MenuItem`.
//! - `state`: file resolution (`resolve_menu`), porting (`port_far_menu`),
//!   creating a fresh file (`create_menu_file`), `UserMenuState`
//!   (browsing, possibly nested, add/remove an item and persist),
//!   `UserMenuPromptState` (collecting `!?...?!` answers before running
//!   an item), `AddUserMenuItemState` (the `Ins` add-item form),
//!   `UserMenuCommandEdit`/`create_command_edit_file`/`finish_command_edit`
//!   (`F4` on an item -- editing just its own command(s) in the real
//!   built-in editor via a scratch file, not the whole
//!   `LitastumMenu.toml`).
//! - `input`: key handling for all of the above, including handing the
//!   finished command list off to `command_line::run_shell_command_lines`.

mod input;
mod parse;
mod state;
mod toml_format;

pub use input::{handle_add_user_menu_item_key, handle_confirm_port_far_menu_key, handle_user_menu_key, handle_user_menu_prompt_key};
pub use parse::MenuItemBody;
pub use state::{create_menu_file, finish_command_edit, resolve_menu, AddUserMenuItemState, MenuFile, UserMenuCommandEdit, UserMenuPromptState, UserMenuState};

// `Prompt`/`MenuItem` have no production caller outside this module --
// `state.rs`'s own code reaches both through `parse` directly. Only
// `ui::user_menu`'s tests build one from outside `explorer` (to
// construct fixtures for rendering tests), so these re-exports (and
// `explorer.rs`'s own further re-export of them) are test-only.
#[cfg(test)]
pub use parse::{MenuItem, Prompt};
