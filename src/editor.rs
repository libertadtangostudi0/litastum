mod bindings;
mod bracket_match;
mod clipboard;
mod editor;
mod editor_keymap;
pub mod find_history;
mod keymap_menu;
mod keymap_mode;
mod line_ending_menu;
mod menu;
mod save_flow;
mod syntax;
mod word_highlight;

pub use editor::{Editor, LineEnding};
pub use save_flow::{handle_confirm_overwrite_key, save_focused_or_ask};
pub use editor_keymap::{handle_confirm_discard_key, handle_editor_key};
pub use keymap_menu::{handle_editor_keymap_menu_key, EditorKeymapMenu};
pub use keymap_mode::EditorKeymapMode;
pub use line_ending_menu::{handle_line_ending_menu_key, EditorLineEndingMenu};
pub use menu::{handle_editor_menu_key, EditorMenu};
#[cfg(test)]
pub use menu::EditorMenuItem;
pub(crate) use editor::fitted_title;
pub(crate) use editor_keymap::close_editor_or_confirm;
pub(crate) use editor_keymap::{cancel_selection, edtui_supports_key, text_key};
