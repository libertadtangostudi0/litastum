use ratatui::{layout::Rect, Frame};

use crate::theming::{MainMenu, PopupStyle, Theme};
use crate::ui::popup;

/// Renders the F9 top menu: whichever level's items are current
/// (`MenuLevel::items`), with the highlighted row picked out.
pub fn draw_main_menu(frame: &mut Frame, area: Rect, menu: &MainMenu, theme: &Theme, style: PopupStyle) {
    let labels = popup::choice_labels(&menu.list, |item| item.label().to_string(), None);
    popup::draw_list_popup(frame, area, theme, style, menu.level.title(), 30, &labels, menu.list.selected_index(), "open", "back");
}
