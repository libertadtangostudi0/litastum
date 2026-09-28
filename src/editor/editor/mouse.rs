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

    /// A mouse event over the editor: a click places the caret, a drag
    /// selects, the wheel scrolls -- all `edtui`'s own mouse handling
    /// (its `mouse-support` feature, already enabled), which maps the
    /// screen position through its own viewport and line wrapping.
    ///
    /// Requested directly, to match VS Code: clicking into the text while
    /// the `Ctrl+F` box is open moves keyboard focus to the text (so the
    /// caret can be moved on from there with the arrows) without closing
    /// the box or losing its match highlight -- `blur_search_box` first.
    ///
    /// `edtui` switches a click that ends a `Visual` selection to
    /// `Normal` (its own vim-shaped default); the `Standard` keymap never
    /// uses `Normal` at all, so that's corrected straight back to its own
    /// `Insert`.
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
