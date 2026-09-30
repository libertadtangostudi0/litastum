use crossterm::event::{MouseEvent, MouseEventKind};
use edtui::EditorMode;

use super::super::keymap_mode::EditorKeymapMode;
use super::Editor;

impl Editor {
    /// Whether screen cell `(column, row)` lies inside the area this
    /// editor was last drawn into -- so a mouse event reaches the editor
    /// only when it actually lands on it, not a linked Markdown preview
    /// drawn beside it (`event_loop::handle_mouse`).
    pub fn contains_screen_position(&self, column: u16, row: u16) -> bool {
        let area = self.view_area;
        column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
    }

    /// A mouse event over the editor -- click, drag-select and wheel are
    /// `edtui`'s own handling. A click while the `Ctrl+F` box is open moves
    /// focus to the text but keeps the box and its match (VS Code). `edtui`
    /// ends a click-selection in `Normal`; `Standard` goes back to `Insert`.
    pub fn mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.blur_search_box();
        }
        self.event_handler.on_mouse_event(mouse, &mut self.state);
        if self.keymap_mode == EditorKeymapMode::Standard && self.state.mode == EditorMode::Normal {
            self.state.mode = EditorMode::Insert;
        }
    }
}
