use ratatui::{layout::Rect, Frame};

use crate::theming::{MainMenu, MenuLevel, PopupStyle, Theme};
use crate::ui::popup;

/// Renders the F9 top menu: whichever level's items are current
/// (`MenuLevel::items`), with the highlighted row picked out.
pub fn draw_main_menu(frame: &mut Frame, area: Rect, menu: &MainMenu, theme: &Theme, style: PopupStyle) {
    let title = match menu.level {
        MenuLevel::Main => " Menu ",
        MenuLevel::Commands => " Commands ",
        MenuLevel::Options => " Options ",
    };
    let labels: Vec<String> = menu.level.items().iter().map(|label| (*label).to_string()).collect();
    popup::draw_list_popup(frame, area, theme, style, title, 30, &labels, menu.selected, "open", "back");
}
