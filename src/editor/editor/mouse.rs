use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use edtui::{EditorMode, Index2};

use super::super::keymap_mode::EditorKeymapMode;
use super::Editor;

/// Lines one wheel event scrolls -- Windows' default per notch. A
/// touchpad's movement reaches us as the same events (litastum's window
/// sends one per three lines of finger travel).
const WHEEL_LINES: isize = 3;

impl Editor {
    /// Whether `(column, row)` is on this editor's top border, between the
    /// corners -- where its path title is drawn.
    pub fn title_row_contains(&self, column: u16, row: u16) -> bool {
        let area = self.view_area;
        row == area.y && column > area.x && column + 1 < area.right()
    }

    /// The top border's inner cells, for drawing over the title.
    pub fn title_area(&self) -> ratatui::layout::Rect {
        let area = self.view_area;
        ratatui::layout::Rect { x: area.x + 1, y: area.y, width: area.width.saturating_sub(2), height: area.height.min(1) }
    }

    /// Whether screen cell `(column, row)` lies inside the area this
    /// editor was last drawn into -- so a mouse event reaches the editor
    /// only when it actually lands on it, not a linked Markdown preview
    /// drawn beside it (`event_loop::handle_mouse`).
    pub fn contains_screen_position(&self, column: u16, row: u16) -> bool {
        let area = self.view_area;
        column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
    }

    /// A mouse event over the editor -- the wheel is ours (`scroll_lines`);
    /// a click and a drag-select go through `edtui` (the selection, its
    /// mode), then land where our own layout says (`text_position_at`):
    /// `edtui`'s kept a click right of a line on its last character. A click
    /// while the `Ctrl+F` box is open moves focus to the text but keeps the
    /// box and its match (VS Code). `edtui` ends a click-selection in
    /// `Normal`; `Standard` goes back to `Insert`.
    pub fn mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.blur_search_box();
        }
        // With a selection, the caret is its end and can't move along:
        // `edtui`'s own scroll stays.
        if self.state.selection.is_none() {
            match mouse.kind {
                MouseEventKind::ScrollDown => return self.scroll_lines(WHEEL_LINES),
                MouseEventKind::ScrollUp => return self.scroll_lines(-WHEEL_LINES),
                _ => {}
            }
        }
        self.event_handler.on_mouse_event(mouse, &mut self.state);
        if self.keymap_mode == EditorKeymapMode::Standard {
            self.place_mouse_caret(mouse);
            if self.state.mode == EditorMode::Normal {
                self.state.mode = EditorMode::Insert;
            }
        }
        if matches!(mouse.kind, MouseEventKind::Down(_) | MouseEventKind::Up(_)) {
            tracing::debug!(kind = ?mouse.kind, column = mouse.column, row = mouse.row, text_left = self.text_left(), caret = ?self.state.cursor, "editor click");
        }
    }

    /// Moves the caret (and a drag's selection end) to where the pointer
    /// is in our layout. A press or release with no selection may land
    /// after a line's end; a selection, inclusive, ends on a character.
    fn place_mouse_caret(&mut self, mouse: MouseEvent) {
        let left = |kind| matches!(kind, MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left));
        if !left(mouse.kind) || !mouse.modifiers.is_empty() {
            return;
        }
        let Some(mut position) = self.text_position_at(mouse.column, mouse.row) else {
            return;
        };
        if let Some(selection) = &mut self.state.selection {
            if mouse.kind != MouseEventKind::Drag(MouseButton::Left) {
                return;
            }
            let len = self.state.lines.len_col(position.row).unwrap_or(0);
            position.col = position.col.min(len.saturating_sub(1));
            selection.end = position;
        }
        self.state.cursor = position;
    }

    /// Scrolls the view `delta` lines (down positive), the caret moving
    /// along on the same screen row. Reported: the wheel sometimes did
    /// nothing -- `edtui` moved only the view, then pulled it back to keep
    /// the caret a few rows off the edge, so scrolling stopped once the
    /// caret got there.
    pub fn scroll_lines(&mut self, delta: isize) {
        let last = self.line_count().saturating_sub(1);
        let (offset_x, top) = self.state.viewport_offset();
        let new_top = top.saturating_add_signed(delta).min(last);
        if new_top == top {
            return;
        }
        let moved = new_top as isize - top as isize;
        let row = self.state.cursor.row.saturating_add_signed(moved).min(last);
        self.state.cursor = Index2::new(row, self.state.cursor.col.min(self.line_len(row)));
        self.state.set_viewport_offset(offset_x, new_top);
    }
}
