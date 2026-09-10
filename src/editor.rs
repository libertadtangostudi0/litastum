mod bindings;
mod clipboard;
mod editor;
mod editor_keymap;
pub mod find_history;
mod syntax;
mod word_highlight;

pub use editor::Editor;
pub use editor_keymap::{handle_confirm_discard_key, handle_editor_key};
