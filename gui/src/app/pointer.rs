//! The mouse: the pointer's cell (and the cell edge nearest it, for the
//! editors' clicks), buttons, the wheel -- reported to the program when it
//! takes them, the tab bar's when over it.

use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::App;
use crate::input::{self, Zoom};
use crate::mouse::{self, MouseAction};
use crate::render;

impl App {
    fn mouse(&self, action: MouseAction) {
        let (mut column, line) = self.pointer_cell;
        let click = matches!(action, MouseAction::Press(MouseButton::Left) | MouseAction::Release(MouseButton::Left));
        if click && mouse::clicks_snap_to_edges(&self.active_screen()) {
            column = self.pointer_edge_column;
        }
        if let Some(bytes) = mouse::report(action, column, line, self.modifiers, self.mode()) {
            self.write(bytes);
        }
    }

    /// The pointer over the tab bar (the top row) is the window's; under
    /// it, the cell it's over goes to the program.
    pub(super) fn pointer_moved(&mut self, x: f64, y: f64) {
        let Some(shown) = &self.shown else {
            return;
        };
        let (origin_x, origin_y) = render::grid_origin(shown.window.inner_size().width, shown.grid.columns, shown.font.cell_width, shown.font.cell_height);
        let column = mouse::column_at(x, origin_x, shown.font.cell_width, shown.grid.columns, false);
        self.pointer_edge_column = mouse::column_at(x, origin_x, shown.font.cell_width, shown.grid.columns, true);
        if y < f64::from(origin_y) {
            self.pointer_on_bar = Some(column);
            return;
        }
        self.pointer_on_bar = None;
        let line = ((y - f64::from(origin_y)).max(0.0) as u32 / shown.font.cell_height) as usize;
        let cell = (column, line.min(shown.grid.lines - 1));
        if cell != self.pointer_cell {
            self.pointer_cell = cell;
            self.mouse(MouseAction::Move { held: self.held_button });
        }
    }

    /// Mouse reports when the program takes them, else arrow keys on the
    /// alternate screen (`input::alternate_scroll`).
    pub(super) fn wheel(&mut self, delta: MouseScrollDelta) {
        let cell_height = self.shown.as_ref().map_or(16, |shown| shown.font.cell_height);
        let lines = self.wheel.steps(delta, cell_height);
        if self.modifiers.control_key() {
            let zoom = if lines > 0 { Zoom::In } else { Zoom::Out };
            for _ in 0..lines.unsigned_abs() {
                self.zoom(zoom);
            }
            return;
        }
        let mode = self.mode();
        if let Some(arrows) = input::alternate_scroll(lines, mode) {
            self.write(arrows);
            return;
        }
        let action = if lines > 0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
        for _ in 0..lines.unsigned_abs() {
            self.mouse(action);
        }
    }


    /// A button pressed or let go: on the tab bar the window's, under it
    /// the program's.
    pub(super) fn mouse_button(&mut self, state: ElementState, button: MouseButton) {
        if let Some(column) = self.pointer_on_bar {
            if state == ElementState::Pressed {
                self.bar_click(column, button);
            }
            return;
        }
        let action = if state == ElementState::Pressed {
            self.held_button = Some(button);
            MouseAction::Press(button)
        } else {
            self.held_button = None;
            MouseAction::Release(button)
        };
        self.mouse(action);
    }
}
