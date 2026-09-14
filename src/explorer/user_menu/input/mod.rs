//! `input.rs` split into one file per `Mode` once it passed the
//! project's own ~500-line decomposition threshold ([[code-conventions]])
//! -- 979 lines, mixing key handling for four different popups
//! (`Mode::UserMenu` browsing/running, `Mode::UserMenuPrompt`,
//! `Mode::ConfirmPortFarMenu`, `Mode::AddUserMenuItem`) in one place.
//! Every symbol that was `pub` at the top of the old file is
//! re-exported here unchanged, so `explorer/user_menu.rs`'s own
//! `pub use input::{...}` list didn't need to change.

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

/// A throwaway `Terminal` for handlers that need one just to satisfy
/// the signature -- never actually drawn to or suspended in these
/// tests, since every test here either stays inside the popup (no
/// command runs) or is documented as covering the "would run" path
/// only up to the point where it hands off to
/// `command_line::run_shell_command_lines` (which needs a real console
/// and isn't exercised directly here, same limitation
/// `command_line::browsing`'s own tests already accept). The handlers'
/// own signature hardcodes `CrosstermBackend<Stdout>` (matching
/// `main.rs`'s real terminal type), not a generic backend, so this has
/// to wrap real stdout too -- harmless here since nothing in these
/// tests ever calls `.draw()` on it.
#[cfg(test)]
fn dummy_terminal() -> ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>> {
    ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout())).unwrap()
}
