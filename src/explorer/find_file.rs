//! F9 → Commands → Find file (also `Alt+F7`, `command_line.rs`): type
//! a filename substring or glob (`*`/`?`), search recursively from the
//! active panel's directory, jump to whichever result gets picked, or
//! `Ctrl+S` to export the full list to a file. Split into
//! `state`/`search`/`export`/`input`/`background` by concern — see
//! `search.rs`'s own doc comment for a known performance caveat in the
//! walk itself, and `background.rs`'s for how the search actually runs
//! (a background thread, live progress, `Esc`-cancel) without blocking
//! the UI.

mod background;
mod export;
mod input;
mod search;
mod state;

pub use background::{is_find_file_search_pending, poll_pending_find_file_search};
pub use input::handle_find_file_key;
pub use state::{FindFileField, FindFilePhase, FindFileState};

// `spawn_search` has no production caller outside `background` itself
// (`input.rs::run_search` calls it, but only from inside this same
// module tree) -- only `ui::find_file`'s own tests need to build a real
// `FindFileState { phase: Searching, pending: Some(_), .. }` from
// outside `explorer`, so this re-export is test-only, same pattern as
// `MarkdownLink`/`Prompt`/`MenuItem` in `explorer.rs`.
#[cfg(test)]
pub use background::spawn_search;
