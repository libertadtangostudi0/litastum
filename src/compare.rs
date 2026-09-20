//! `Alt+F5`: a two-file, side-by-side, GitHub-diff-colored comparer --
//! phase 1 of `TODO/file-compare.md`'s own design (read-only; no 3-way
//! merge/conflict resolution yet, see that document for phase 2). Both
//! files come from the two panels directly (the active panel's own
//! selected file on the left, the *other* panel's own selected file on
//! the right) -- no file picker in this phase.
//!
//! Split by concern like every other multi-file feature in this
//! codebase: `diff` (the actual line-level comparison, `similar`-backed),
//! `line_ending` (the F9 -> Line endings feature and its own per-line
//! `CRLF`/`LF` detection), `state` (`CompareState`/`ComparePane`, the
//! diff's own display lines and metadata -- no `edtui::EditorState` is
//! kept here at all, see `ComparePane`'s own doc comment for why),
//! `input` (key handling), `menu`/`line_ending_menu` (the two-level F9
//! menu, mirroring `editor::menu`/`editor::keymap_menu`'s own shape).

mod diff;
mod input;
mod line_ending;
mod line_ending_menu;
mod menu;
mod state;

pub use diff::DiffLineKind;
pub use input::handle_compare_key;
pub use line_ending::LineEndingDisplay;
pub use line_ending_menu::{handle_compare_line_ending_menu_key, CompareLineEndingMenu};
pub use menu::{handle_compare_menu_key, CompareMenu};
pub use state::{ComparePane, CompareState};
