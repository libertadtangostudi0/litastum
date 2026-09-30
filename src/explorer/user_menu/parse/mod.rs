//! Menu items, plus the three parsing concerns:
//! - `dsl`: reads `FarMenu.ini` (porting only);
//! - `substitution`: Far's `!...!` "meta-symbols" and litastum's `{{...}}`
//!   (named so to avoid confusion with Far's recorded key macros, which
//!   litastum doesn't have);
//! - `prompts`: `!?Label?Default!`/`{{prompt:...}}`, one question per
//!   unique label, shared by both syntaxes.

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
    /// The character before the item's `:` (`"s: status"` -> `Some('s')`),
    /// shown in the list and a case-insensitive shortcut
    /// (`UserMenuState::select_by_hotkey`). Far's `F1`..`F24` hotkeys aren't
    /// recognized: `F5: refresh` reads as a hotkey-less item.
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
