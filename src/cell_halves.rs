//! Which half of a cell a click is in -- in any terminal, not only
//! litastum's window (requested: in Windows Terminal a click on a letter's
//! right half put the caret before it). A terminal reports only the cell,
//! so litastum reads the pointer's pixel position itself
//! (`GetPhysicalCursorPos`) and learns where the cells are on screen from
//! the pointer's moves: a move into the next cell is a cell edge crossed,
//! at about the pixel the pointer is at. A line fitted through those edges
//! gives the cells' left edge and width; a click in a cell's right half is
//! then reported as the next cell, as litastum's window does for its
//! editors (`gui/src/mouse.rs::column_at`). Windows only; elsewhere, and
//! before enough moves were seen, clicks stay as reported.
//! History: docs/history/editor-rendering.md.

use std::collections::VecDeque;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

/// Edges kept: the latest, so a moved window or a new font is soon
/// forgotten.
const MAX_EDGES: usize = 48;

/// Edges and columns needed before a fit is trusted.
const MIN_EDGES: usize = 4;
const MIN_COLUMN_SPREAD: f64 = 3.0;

/// Edges off the fit in a row that mean the cells moved (the window, a
/// new font), not a late event.
const MISSES_TO_RELEARN: u8 = 3;

/// A cell narrower or wider than this (pixels) is a bad fit.
const CELL_WIDTH_RANGE: std::ops::RangeInclusive<f64> = 3.0..=200.0;


/// The cell edges seen so far, as `(edge column, pixel x)` -- edge `c` is
/// the left edge of column `c` -- apart for moves right and left: events
/// come a little late, so moving right the pointer is a little past an
/// edge and moving left a little before it.
#[derive(Debug, Default)]
pub struct CellHalves {
    edges: [VecDeque<(f64, f64)>; 2],
    last_column: Option<u16>,
    /// Edges off the fit in a row, each way.
    misses: [u8; 2],
}

impl CellHalves {
    /// A move of the pointer to `column`, at pixel `x`. Into the next cell
    /// to the right, `x` is just past that cell's left edge; to the left,
    /// just before the right edge of the cell it entered. A jump over
    /// several cells says nothing about an edge. An edge far off the fit is
    /// skipped (a stray late event); several in a row -- the window moved,
    /// the font changed -- drop what was learned.
    pub fn note_move(&mut self, column: u16, x: f64) {
        let previous = self.last_column.replace(column);
        let (edge, way) = match previous {
            Some(previous) if column == previous.wrapping_add(1) => (column, 0),
            Some(previous) if previous == column.wrapping_add(1) => (previous, 1),
            _ => return,
        };
        if let Some((left, width)) = line(&self.edges[way]) {
            let predicted = left + f64::from(edge) * width;
            if (x - predicted).abs() > width / 2.0 {
                self.misses[way] += 1;
                if self.misses[way] < MISSES_TO_RELEARN {
                    return;
                }
                self.edges = Default::default();
            }
        }
        self.misses[way] = 0;
        let edges = &mut self.edges[way];
        if edges.len() == MAX_EDGES {
            edges.pop_front();
        }
        edges.push_back((f64::from(edge), x));
    }

    /// The column to report for a click in `column` at pixel `x`: the next
    /// one when `x` is in the cell's right half. As reported when the fit
    /// isn't there yet or puts `x` well outside `column`.
    pub fn click_column(&self, column: u16, x: f64) -> u16 {
        let Some((left, width)) = self.fit() else {
            return column;
        };
        let within = (x - (left + f64::from(column) * width)) / width;
        if (0.5..1.25).contains(&within) {
            column.saturating_add(1)
        } else {
            column
        }
    }

    /// The cells' left edge (column 0) and width in pixels: halfway between
    /// the lines of the moves right and left, which are late in opposite
    /// directions; one of them alone until both are known.
    fn fit(&self) -> Option<(f64, f64)> {
        match (line(&self.edges[0]), line(&self.edges[1])) {
            (Some(right), Some(left)) => Some(((right.0 + left.0) / 2.0, (right.1 + left.1) / 2.0)),
            (one, other) => one.or(other),
        }
    }
}


/// A least-squares line `x = left + column * width` through `edges`, once
/// there are enough of them, far enough apart, for a plausible width.
fn line(edges: &VecDeque<(f64, f64)>) -> Option<(f64, f64)> {
    if edges.len() < MIN_EDGES {
        return None;
    }
    let count = edges.len() as f64;
    let mean_column = edges.iter().map(|edge| edge.0).sum::<f64>() / count;
    let mean_x = edges.iter().map(|edge| edge.1).sum::<f64>() / count;
    let (lowest, highest) = edges.iter().fold((f64::MAX, f64::MIN), |(low, high), edge| (low.min(edge.0), high.max(edge.0)));
    if highest - lowest < MIN_COLUMN_SPREAD {
        return None;
    }
    let spread = edges.iter().map(|edge| (edge.0 - mean_column).powi(2)).sum::<f64>();
    let width = edges.iter().map(|edge| (edge.0 - mean_column) * (edge.1 - mean_x)).sum::<f64>() / spread;
    CELL_WIDTH_RANGE.contains(&width).then_some((mean_x - width * mean_column, width))
}


/// `mouse` as litastum should handle it: a left click on an editor screen
/// (`on_editor`) reported at the cell edge nearest the pointer; every move
/// teaches `halves` where the cells are. Unchanged in litastum's own
/// window, which already reports clicks that way.
pub fn adjust(halves: &mut CellHalves, mouse: MouseEvent, on_editor: bool) -> MouseEvent {
    if crate::image_host::host_cell_size().is_some() {
        return mouse;
    }
    let Some(x) = pointer_x() else {
        return mouse;
    };
    match mouse.kind {
        MouseEventKind::Moved | MouseEventKind::Drag(_) => {
            halves.note_move(mouse.column, x);
            mouse
        }
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) if on_editor => {
            let column = halves.click_column(mouse.column, x);
            tracing::debug!(reported = mouse.column, column, x, fit = ?halves.fit(), "click: cell half");
            MouseEvent { column, ..mouse }
        }
        _ => mouse,
    }
}


/// The pointer's horizontal position on screen, in physical pixels.
/// Never in tests: a test's clicks can't depend on where the real
/// pointer is.
#[cfg(all(windows, not(test)))]
fn pointer_x() -> Option<f64> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetPhysicalCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: writes one POINT we own.
    let ok = unsafe { GetPhysicalCursorPos(&mut point) };
    (ok != 0).then_some(f64::from(point.x))
}

#[cfg(any(not(windows), test))]
fn pointer_x() -> Option<f64> {
    None
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Cells 9 pixels wide from pixel 100: column `c` spans
    /// `100 + 9c .. 100 + 9c + 9`.
    const LEFT: f64 = 100.0;
    const WIDTH: f64 = 9.0;

    fn pixel(column: u16, within: f64) -> f64 {
        LEFT + (f64::from(column) + within) * WIDTH
    }

    /// The pointer sweeps right then left across `columns`, seen a little
    /// late (`lag` pixels on in the direction of the move), as events are.
    fn swept(columns: std::ops::Range<u16>, lag: f64) -> CellHalves {
        let mut halves = CellHalves::default();
        for column in columns.clone() {
            halves.note_move(column, pixel(column, 0.0) + lag);
        }
        for column in columns.rev().skip(1) {
            halves.note_move(column, pixel(column + 1, 0.0) - 1.0 - lag);
        }
        halves
    }

    #[test]
    fn nothing_changes_before_the_cells_are_known() {
        let mut halves = CellHalves::default();
        assert_eq!(halves.click_column(5, pixel(5, 0.9)), 5, "no moves yet");
        halves.note_move(5, pixel(5, 0.0));
        halves.note_move(6, pixel(6, 0.0));
        assert_eq!(halves.click_column(5, pixel(5, 0.9)), 5, "one edge isn't enough");
    }

    /// Requested: a click on a letter's right half puts the caret after it.
    #[test]
    fn a_click_in_a_cells_right_half_is_the_next_cell() {
        let halves = swept(10..20, 2.0);
        for column in 10..20 {
            assert_eq!(halves.click_column(column, pixel(column, 0.1)), column, "{column}: left edge");
            assert_eq!(halves.click_column(column, pixel(column, 0.4)), column, "{column}: left half");
            assert_eq!(halves.click_column(column, pixel(column, 0.6)), column + 1, "{column}: right half");
            assert_eq!(halves.click_column(column, pixel(column, 0.95)), column + 1, "{column}: right edge");
        }
    }

    /// Edges learned in one part of the line hold for the whole of it.
    #[test]
    fn the_fit_reaches_columns_never_crossed() {
        let halves = swept(3..9, 1.0);
        assert_eq!(halves.click_column(60, pixel(60, 0.3)), 60);
        assert_eq!(halves.click_column(60, pixel(60, 0.7)), 61);
    }

    /// Every cell width a font size can give, events seen with lag.
    #[test]
    fn every_cell_width_is_learned() {
        for width in 5..=40u16 {
            let width = f64::from(width);
            let mut halves = CellHalves::default();
            let at = |column: u16, within: f64| 37.0 + (f64::from(column) + within) * width;
            for column in 0..12 {
                halves.note_move(column, at(column, 0.0) + width * 0.2);
            }
            for column in (0..11).rev() {
                halves.note_move(column, at(column + 1, 0.0) - 1.0 - width * 0.2);
            }
            for column in 0..30 {
                assert_eq!(halves.click_column(column, at(column, 0.3)), column, "width {width}, column {column}");
                assert_eq!(halves.click_column(column, at(column, 0.7)), column + 1, "width {width}, column {column}");
            }
        }
    }

    /// A jump over several cells isn't an edge.
    #[test]
    fn a_jump_teaches_nothing() {
        let mut halves = CellHalves::default();
        for column in [0, 5, 10, 20, 2] {
            halves.note_move(column, 999.0);
        }
        assert!(halves.edges.iter().all(VecDeque::is_empty));
    }

    /// The window moved: the old edges are dropped, the new place learned.
    #[test]
    fn a_moved_window_is_relearned() {
        let mut halves = swept(10..20, 1.0);
        let moved = |column: u16, within: f64| pixel(column, within) + 300.0;
        for column in 10..20 {
            halves.note_move(column, moved(column, 0.0) + 1.0);
        }
        for column in (10..19).rev() {
            halves.note_move(column, moved(column + 1, 0.0) - 2.0);
        }
        assert_eq!(halves.click_column(14, moved(14, 0.7)), 15);
        assert_eq!(halves.click_column(14, moved(14, 0.3)), 14);
    }

    /// One late event far off the edges learned is skipped, not learned.
    #[test]
    fn a_stray_edge_is_skipped() {
        let mut halves = swept(10..20, 1.0);
        halves.note_move(19, pixel(19, 0.0) + 30.0);
        halves.note_move(20, pixel(20, 0.0) + 1.0);
        assert_eq!(halves.click_column(14, pixel(14, 0.3)), 14);
        assert_eq!(halves.click_column(14, pixel(14, 0.7)), 15);
    }

    /// A pointer far outside the reported cell means the fit is off: the
    /// cell stays as reported.
    #[test]
    fn a_click_the_fit_disagrees_with_stays() {
        let halves = swept(10..20, 1.0);
        assert_eq!(halves.click_column(14, pixel(30, 0.7)), 14);
    }
}
