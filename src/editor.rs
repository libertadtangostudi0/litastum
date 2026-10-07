mod bindings;
mod bracket_match;
mod clipboard;
mod editor;
mod editor_keymap;
pub mod find_history;
mod keymap_menu;
mod keymap_mode;
mod menu;
mod syntax;
mod word_highlight;

pub use editor::Editor;
pub use editor_keymap::{handle_confirm_discard_key, handle_editor_key};
pub use keymap_menu::{handle_editor_keymap_menu_key, EditorKeymapMenu};
pub use keymap_mode::EditorKeymapMode;
pub use menu::{handle_editor_menu_key, EditorMenu};
#[cfg(test)]
pub use menu::EditorMenuItem;
pub(crate) use editor::fitted_title;
pub(crate) use editor_keymap::close_editor_or_confirm;
pub(crate) use editor_keymap::{cancel_selection, edtui_supports_key, text_key};
