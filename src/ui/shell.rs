use ratatui::{layout::Rect, Frame};

use crate::app::ShellMenu;
use crate::command_line::ShellProfile;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the `Ctrl+P` shell-profile picker popup.
pub fn draw_shell_menu(frame: &mut Frame, area: Rect, menu: &ShellMenu, profiles: &[ShellProfile], theme: &Theme, style: PopupStyle) {
    let labels: Vec<String> = profiles.iter().map(|profile| profile.name.clone()).collect();
    popup::draw_list_popup(frame, area, theme, style, " Shell ", 36, &labels, menu.selected, "select", "cancel");
}
