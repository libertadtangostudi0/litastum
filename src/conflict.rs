//! `Alt+F5` on the four files an SVN merge conflict leaves behind: a
//! three-way resolver on top (`.working` | the file with conflict markers
//! | `.merge-right`), and the incoming change (`.merge-left` ->
//! `.merge-right`) as an ordinary Compare below, after Araxis Merge.
//! History: docs/history/conflict-resolver.md.
//! - `files`: recognizes the four files;
//! - `markers`: finds the conflicts (`<<<<<<<` ... `>>>>>>>`) in a text;
//! - `state`: the five editors and which one has focus;
//! - `input`: resolver commands first, the rest to the focused editor.

mod files;
mod input;
mod markers;
mod state;

pub use files::detect;
pub use input::{handle_conflict_key, open_resolver};
pub use markers::{ConflictRegion, RowRole};
pub use state::{ConflictState, Pane};
#[cfg(test)]
pub(crate) use state::tests as state_tests;
