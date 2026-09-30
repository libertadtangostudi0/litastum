//! F9 -> Commands -> Find file (`Alt+F7`): a name substring or glob, plus
//! optional "Text to find", searched recursively from the active panel;
//! jump to a result, or `Ctrl+S` to export the list. Split into `state`,
//! `search` (the engine), `background` (thread, progress, cancel),
//! `export`, `input` (`typing`/`results`) and `history`.

mod background;
mod export;
pub mod history;
mod input;
mod search;
mod state;

pub use background::{is_find_file_search_pending, poll_pending_find_file_search};
pub use input::handle_find_file_key;
pub use state::{FindFileField, FindFilePhase, FindFileState};

// `spawn_search` has no production caller outside `background` itself
// (`input/typing.rs::run_search` calls it, but only from inside this
// same module tree) -- only `ui::find_file`'s own tests need to build a real
// `FindFileState { phase: Searching, pending: Some(_), .. }` from
// outside `explorer`, so this re-export is test-only, same pattern as
// `MarkdownLink`/`Prompt`/`MenuItem` in `explorer.rs`.
#[cfg(test)]
pub use background::spawn_search;
