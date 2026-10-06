//! `Alt+F5`: a side-by-side, fully editable file comparer with GitHub-style
//! diff colors, loosely after Far's `merge.exe`; design in
//! `TODO/file-compare.md`. Left is the active panel's selected file,
//! right the other panel's.
//! - `diff`: the live line diff, recomputed every frame -- classifies rows
//!   and maps between sides; never inserts filler into a buffer;
//! - `line_ending`: per-line `CRLF`/`LF` detection (F9 -> Line endings);
//! - `state`: two real `Editor`s and which has focus;
//! - `path_edit`: a pane's path title as an editable field;
//! - `input`: Compare commands first, the rest to the focused editor;
//! - `menu`/`line_ending_menu`: the F9 menu.

mod diff;
mod diff_cache;
mod input;
mod line_ending;
mod line_ending_menu;
mod menu;
mod path_edit;
mod state;

pub use diff::{hunk_start_rows, inline_changes, map_real_row, DiffLineKind, DiffLines};
pub use diff_cache::DiffCache;
pub use input::{handle_compare_confirm_discard_key, handle_compare_key};
pub use line_ending::{LineEnding, LineEndingDisplay};
pub use line_ending_menu::{handle_compare_line_ending_menu_key, CompareLineEndingMenu};
pub use menu::{handle_compare_menu_key, CompareMenu};
#[cfg(test)]
pub use menu::CompareMenuItem;
pub use path_edit::{PathEdit, PathEditKey};
pub use state::{CompareState, Side};
