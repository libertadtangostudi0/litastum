//! `F2` — Far Manager's own "user menu": a per-directory list of
//! shortcuts to shell commands, defined in a `LitastumMenu.ini` file
//! (or migrated once from a compatible `FarMenu.ini`, if that's all
//! that's there — see `state::resolve_menu_file`). Split into `parse`/
//! `state`/`input` by concern, same shape as `find_file.rs`:
//! - `parse`: the file's own nested-block grammar, plus the `!&`/
//!   `!?Label?Default!` macro substitution — pure, no I/O.
//! - `state`: file resolution/migration, `UserMenuState` (browsing,
//!   possibly-nested), `UserMenuPromptState` (collecting `!?...?!`
//!   answers before running an item).
//! - `input`: key handling for both of the above, including handing
//!   the finished command list off to
//!   `command_line::run_shell_command_lines`.

mod input;
mod parse;
mod state;

pub use input::{handle_user_menu_key, handle_user_menu_prompt_key};
pub use parse::MenuItemBody;
pub use state::{create_menu_file, UserMenuPromptState, UserMenuState};

// `Prompt` has no production caller outside this module -- `state.rs`'s
// own `UserMenuPromptState::new` reaches it through `parse` directly.
// Only `ui::user_menu`'s tests build one from outside `explorer`, so
// this re-export (and `explorer.rs`'s own further re-export of it) is
// test-only.
#[cfg(test)]
pub use parse::Prompt;
