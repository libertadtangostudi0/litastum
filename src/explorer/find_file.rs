//! F9 → Commands → Find file (also `Alt+F7`, `command_line.rs`): type
//! a filename substring or glob (`*`/`?`), search recursively from the
//! active panel's directory, jump to whichever result gets picked, or
//! `Ctrl+S` to export the full list to a file. Split into
//! `state`/`search`/`export`/`input` by concern — see `search.rs`'s own
//! doc comment for a known performance caveat in the search itself.

mod export;
mod input;
mod search;
mod state;

pub use input::handle_find_file_key;
pub use state::{FindFilePhase, FindFileState};
