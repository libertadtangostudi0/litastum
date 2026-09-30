//! Key handling, one file per popup: browsing, prompt, port confirmation,
//! add-item form. Re-exports the public handlers.

mod add_item;
mod browsing;
mod confirm_port;
mod prompt;

pub use add_item::handle_add_user_menu_item_key;
pub use browsing::handle_user_menu_key;
pub use confirm_port::handle_confirm_port_far_menu_key;
pub use prompt::handle_user_menu_prompt_key;

/// Shared by every submodule's own `#[cfg(test)] mod tests` below --
/// a plain, non-`pub` item here is already visible to every descendant
/// module, no visibility juggling needed to share these two test
/// helpers across them (same pattern `state/mod.rs::scratch_dir`
/// already uses).
#[cfg(test)]
fn scratch_dir() -> std::path::PathBuf {
    crate::test_support::unique_scratch_dir("user-menu-input")
}

