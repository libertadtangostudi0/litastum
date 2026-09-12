//! `FarMenu.ini`'s own nested-block DSL, plus macro substitution for a
//! `Commands` item's own command strings -- split into `dsl`/
//! `substitution`/`prompts` by concern once this file passed the
//! ~500-line decomposition threshold
//! (`.claude/rules/code-conventions.md`), same shape as `user_menu.rs`'s
//! own split one level up:
//! - `dsl`: reads a real `FarMenu.ini` into `MenuItem`s -- only used
//!   for one-time porting, never for litastum's own file.
//! - `substitution`: substitutes both real Far Manager's own `!...!`
//!   family (what Far's own docs call "meta-symbols," not to be
//!   confused with Far's *other*, much bigger macro feature --
//!   scripted/recorded key sequences, `F11`/macro editor -- which this
//!   file has nothing to do with and litastum doesn't implement; named
//!   `substitution.rs` rather than `macros.rs` specifically to avoid
//!   that collision if a real macro-recording feature is ever added
//!   later) and litastum's own `{{...}}` family into a `Commands`
//!   item's strings, run every time an item is executed regardless of
//!   which file format produced it.
//! - `prompts`: the `!?Label?Default!`/`{{prompt:...}}` placeholder --
//!   asks the user for a value once per unique label before running,
//!   shared machinery for both macro syntaxes since the interactive
//!   part (`Mode::UserMenuPrompt`) doesn't care which one was used.
//!
//! `MenuItem`/`MenuItemBody` live here, at the top, since they're the
//! one shape all three submodules (and `toml_format.rs`, `state.rs`,
//! `input.rs` outside this directory) actually pass around.

mod dsl;
mod prompts;
mod substitution;

pub use dsl::parse;
pub use prompts::{extract_prompts, substitute_prompts, Prompt};
pub use substitution::{substitute_macros, MacroContext, PanelMacroContext};


/// One parsed menu entry -- built either by `dsl::parse` (reading a
/// real `FarMenu.ini`, for one-time porting) or by
/// `toml_format::parse_toml` (reading litastum's own
/// `LitastumMenu.toml`). Both file formats resolve to the same
/// in-memory shape, which is what `state.rs`/`input.rs` actually
/// browse and execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// The single character before the item's own `:` (`"s: status"` ->
    /// `Some('s')`), shown as a prefix in the list but not currently
    /// wired up as an instant-select shortcut -- matches this
    /// codebase's own existing precedent (`TODO/f9-menu.md`'s "no
    /// keyboard shortcut letters" gap on the F9 menu too); `Up`/`Down`/
    /// `Enter` is enough for now. `F1`..`F24`-style hotkeys (real Far
    /// Manager also allows those) aren't recognized as hotkeys at all
    /// here -- a line like `F5: refresh` doesn't match the single-char
    /// prefix rule below, so it falls through to being read as a
    /// hotkey-less item titled `F5: refresh`'s remainder, a known,
    /// narrow gap rather than full parity with every hotkey Far allows.
    pub hotkey: Option<char>,
    pub title: String,
    pub body: MenuItemBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemBody {
    /// Every line after the header, in order, until the next header
    /// line, a closing `}`, or end of file -- run in sequence when this
    /// item is selected (`command_line::run_shell_command_lines`).
    Commands(Vec<String>),
    /// A `{ ... }` block right after the header -- its own nested
    /// sequence of items, entered by selecting this one.
    Submenu(Vec<MenuItem>),
}
