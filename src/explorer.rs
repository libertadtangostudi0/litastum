mod command;
mod confirm;
mod drive_menu;
mod entry;
mod find_file;
mod fs_ops;
mod image_preview;
mod keymap;
mod panel;
mod system_open;
mod user_menu;

pub use command::execute;
pub use confirm::{handle_confirm_delete_key, handle_confirm_transfer_key};
pub use drive_menu::{format_bytes, handle_drive_menu_key, DriveMenu};
pub use entry::{Entry, HighlightRole};
pub use find_file::{handle_find_file_key, FindFilePhase, FindFileState};
pub use image_preview::{handle_image_preview_key, ImagePreviewState};
pub use keymap::{resolve, Command};
pub use panel::Panel;
pub use user_menu::{
    handle_add_user_menu_item_key, handle_confirm_port_far_menu_key, handle_user_menu_key, handle_user_menu_prompt_key, finish_command_edit, resolve_menu,
    AddUserMenuItemState, MenuFile, MenuItemBody, UserMenuCommandEdit, UserMenuPromptState, UserMenuState,
};
// `Prompt`/`MenuItem` have no production caller of their own --
// `user_menu`'s own code reaches both through its internal `parse`
// module directly. Only `ui::user_menu`'s tests build one from outside
// `explorer`, so these re-exports are test-only.
#[cfg(test)]
pub use user_menu::{MenuItem, Prompt};
