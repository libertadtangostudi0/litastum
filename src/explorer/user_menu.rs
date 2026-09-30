//! `F2` -- Far Manager's user menu: per-directory shortcuts to shell
//! commands, stored in `LitastumMenu.toml` or ported from a `FarMenu.ini`
//! on confirmation (`Overlay::ConfirmPortFarMenu`).
//! - `parse`: reads `FarMenu.ini`'s DSL (porting only), and substitutes
//!   Far's `!...!` and litastum's `{{...}}` macros into commands. Pure.
//! - `toml_format`: the `LitastumMenu.toml` shape and its `MenuItem`
//!   conversion.
//! - `state`: file resolution, porting, the fresh-file template, browsing
//!   and editing (`UserMenuState`), prompts, the add-item form and the
//!   `F4` command-edit flow.
//! - `input`: key handling for all of the above.

mod input;
mod parse;
mod state;
mod toml_format;

pub use input::{handle_add_user_menu_item_key, handle_confirm_port_far_menu_key, handle_user_menu_key, handle_user_menu_prompt_key};
pub use parse::MenuItemBody;
pub use state::{common_menu_dir, create_menu_file, finish_command_edit, resolve_menu, AddUserMenuItemState, MenuFile, UserMenuCommandEdit, UserMenuPromptState, UserMenuState};

// `Prompt`/`MenuItem` have no production caller outside this module --
// `state.rs`'s own code reaches both through `parse` directly. Only
// `ui::user_menu`'s tests build one from outside `explorer` (to
// construct fixtures for rendering tests), so these re-exports (and
// `explorer.rs`'s own further re-export of them) are test-only.
#[cfg(test)]
pub use parse::{MenuItem, Prompt};
