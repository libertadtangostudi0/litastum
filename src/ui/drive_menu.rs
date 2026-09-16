use ratatui::{layout::Rect, Frame};

use crate::explorer::{format_bytes, DriveMenu};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the `Alt+F1`/`Alt+F2` "change drive" popup: one row per
/// drive (letter, type, total/free space — `"—"` for a size that
/// couldn't be read, e.g. an empty removable/CD drive).
pub fn draw_drive_menu(frame: &mut Frame, area: Rect, menu: &DriveMenu, theme: &Theme, style: PopupStyle) {
    let labels: Vec<String> = menu
        .drives
        .iter()
        .map(|drive| {
            let total = drive.total_bytes.map_or_else(|| "—".to_string(), format_bytes);
            let free = drive.free_bytes.map_or_else(|| "—".to_string(), format_bytes);
            format!("{:<4}{:<10}{total:>10}{free:>10}", drive.label, drive.kind)
        })
        .collect();
    popup::draw_list_popup(frame, area, theme, style, " Change drive ", 50, &labels, menu.selected, "select", "cancel");
}
