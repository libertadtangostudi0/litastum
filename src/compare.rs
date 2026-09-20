//! `Alt+F5`: a two-file, side-by-side, **fully editable**, GitHub-diff-
//! colored comparer, modeled loosely on Far Manager's own `merge.exe`
//! but reusing this app's own `Editor`/themes -- see `TODO/file-compare.md`
//! for the full design. Both files come from the two panels directly
//! (the active panel's own selected file on the left, the *other*
//! panel's own selected file on the right) -- no file picker.
//!
//! Split by concern like every other multi-file feature in this
//! codebase: `diff` (the live, `similar`-backed line-level comparison,
//! recomputed every frame from both panes' current text -- classifies
//! rows and maps between the two sides, nothing more; neither pane's
//! real buffer ever has synthetic filler lines injected into it),
//! `line_ending` (the F9 -> Line endings feature and its own per-line
//! `CRLF`/`LF` detection), `state` (`CompareState`, two real, independent
//! `editor::Editor` sessions plus which one currently has focus -- see
//! its own doc comment for how the *unfocused* pane's viewport stays
//! diff-aligned with the focused one without ever touching either
//! pane's real content), `input` (key handling: Compare-level commands
//! first, everything else forwarded into the focused `Editor`),
//! `menu`/`line_ending_menu` (the two-level F9 menu, mirroring
//! `editor::menu`/`editor::keymap_menu`'s own shape).

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
pub use state::{CompareState, Side};
