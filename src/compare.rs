//! `Alt+F5`: a side-by-side, fully editable file comparer with GitHub-style
//! diff colors, loosely after Far's `merge.exe`; design in
//! `TODO/file-compare.md`. Left is the active panel's selected file,
//! right the other panel's.
//! - `diff`: the live line diff, recomputed every frame -- classifies rows
//!   and maps between sides; never inserts filler into a buffer;
//! - `line_ending`: per-line `CRLF`/`LF` detection (F9 -> Line endings);
//! - `state`: two real `Editor`s and which has focus;
//! - `input`: Compare commands first, the rest to the focused editor;
//! - `menu`/`line_ending_menu`: the F9 menu.

mod diff;
mod input;
mod line_ending;
mod line_ending_menu;
mod menu;
mod state;

pub use diff::{compute, map_real_row, DiffLineKind};
pub use input::{handle_compare_confirm_discard_key, handle_compare_key};
pub use line_ending::{LineEnding, LineEndingDisplay};
pub use line_ending_menu::{handle_compare_line_ending_menu_key, CompareLineEndingMenu};
pub use menu::{handle_compare_menu_key, CompareMenu};
#[cfg(test)]
pub use menu::CompareMenuItem;
pub use state::{CompareState, Side};
