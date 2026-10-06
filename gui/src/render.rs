use std::ops::Range;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermDamage};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::colors;
use crate::font::{CellFont, FontStyle, GlyphMask};
use crate::images::PlacedImage;

/// What a frame is drawn for: the window's size in pixels, where the
/// grid's top-left cell starts (`grid_origin`), and whether a blinking
/// cursor is in its shown half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub origin: (u32, u32),
    pub cursor_visible: bool,
}


/// Draws the terminal into a back buffer of its own and keeps it between
/// frames, redrawing only the lines that changed: the terminal's damage,
/// the cursor's old and new line, and the lines under images. Typing a
/// character redraws a line or two rather than every cell and glyph of
/// the window, which every frame used to.
pub struct Renderer {
    back: Vec<u32>,
    view: Option<View>,
    /// Redraw everything next frame (`invalidate`).
    full: bool,
    cursor_line: Option<usize>,
    image_lines: Vec<Range<usize>>,
    background: Option<Rgb>,
}

impl Renderer {
    pub fn new() -> Self {
        Self { back: Vec::new(), view: None, full: true, cursor_line: None, image_lines: Vec::new(), background: None }
    }

    /// The next frame redraws everything -- after the font (cell size)
    /// changed, which nothing in the terminal reports.
    pub fn invalidate(&mut self) {
        self.full = true;
    }

    /// Brings the back buffer up to date with `term` and returns it, one
    /// `0x00RRGGBB` per pixel, row by row. Drops images written over
    /// (`drop_overwritten_images`) on the way.
    pub fn draw<T: EventListener>(&mut self, term: &mut Term<T>, font: &mut CellFont, images: &mut Vec<PlacedImage>, view: View) -> &[u32] {
        let cell = (font.cell_width, font.cell_height);
        if self.view.map(|old| (old.width, old.height, old.origin)) != Some((view.width, view.height, view.origin)) {
            self.back = vec![0; (view.width * view.height) as usize];
            self.full = true;
        }
        let lines = term.screen_lines();
        let mut dirty = vec![false; lines];
        match term.damage() {
            TermDamage::Full => self.full = true,
            TermDamage::Partial(damaged) => {
                for bounds in damaged {
                    if let Some(line) = dirty.get_mut(bounds.line) {
                        *line = true;
                    }
                }
            }
        }
        term.reset_damage();

        let background = colors::resolve(Color::Named(NamedColor::Background), term.colors());
        if self.background != Some(background) {
            self.full = true;
        }
        let mut canvas = Canvas { pixels: &mut self.back, width: view.width, height: view.height };
        if self.full {
            // The margins take the program's background (`OSC 11`), like
            // the cells it leaves unpainted.
            canvas.fill(0, 0, view.width, view.height, background);
            dirty.fill(true);
        }

        let content = term.renderable_content();
        let cursor_line = usize::try_from(content.cursor.point.line.0 + content.display_offset as i32).ok();
        for line in [self.cursor_line, cursor_line].into_iter().flatten() {
            mark(&mut dirty, line..line + 1);
        }
        drop_overwritten_images(term, images, cell);
        let image_lines: Vec<Range<usize>> = images.iter().map(|image| image.line..image.line + image.cells(cell).1).collect();
        for range in self.image_lines.iter().chain(&image_lines) {
            mark(&mut dirty, range.clone());
        }

        for (line, _) in dirty.iter().enumerate().filter(|(_, dirty)| **dirty) {
            draw_line(&mut canvas, term, font, line, view.origin);
        }
        for image in images.iter() {
            canvas.blit(view.origin.0 + image.column as u32 * cell.0, view.origin.1 + image.line as u32 * cell.1, image);
        }
        if view.cursor_visible {
            if let Some(line) = cursor_line {
                let (x, y) = (view.origin.0 + content.cursor.point.column.0 as u32 * cell.0, view.origin.1 + line as u32 * cell.1);
                draw_cursor(&mut canvas, content.cursor.shape, x, y, cell, colors::resolve(Color::Named(NamedColor::Cursor), content.colors));
            }
        }

        self.view = Some(view);
        self.full = false;
        self.cursor_line = cursor_line;
        self.image_lines = image_lines;
        self.background = Some(background);
        &self.back
    }
}


fn mark(dirty: &mut [bool], range: Range<usize>) {
    let end = range.end.min(dirty.len());
    if range.start < end {
        dirty[range.start..end].fill(true);
    }
}


/// One grid line: every cell's background, glyph and decorations.
fn draw_line<T>(canvas: &mut Canvas, term: &Term<T>, font: &mut CellFont, line: usize, origin: (u32, u32)) {
    let (cell_width, cell_height) = (font.cell_width, font.cell_height);
    let grid = term.grid();
    let row = &grid[Line(line as i32 - grid.display_offset() as i32)];
    let colors = term.colors();
    let y = origin.1 + line as u32 * cell_height;
    for column in 0..grid.columns() {
        let cell = &row[Column(column)];
        let x = origin.0 + column as u32 * cell_width;
        let mut foreground = colors::resolve(cell.fg, colors);
        let mut background = colors::resolve(cell.bg, colors);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        if cell.flags.contains(Flags::DIM) {
            foreground = mix(background, foreground, 0xaa);
        }
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            // Its glyph is the wide character's to the left.
            continue;
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { cell_width * 2 } else { cell_width };
        canvas.fill(x, y, width, cell_height, background);
        if cell.flags.contains(Flags::HIDDEN) {
            continue;
        }
        let style = FontStyle { bold: cell.flags.intersects(Flags::BOLD), italic: cell.flags.intersects(Flags::ITALIC) };
        if let Some(mask) = font.mask(cell.c, style) {
            canvas.draw_mask(x as i32, y as i32, &mask, foreground, y..y + cell_height);
        }
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            canvas.fill(x, y + cell_height - 2, width, 1, foreground);
        }
        if cell.flags.contains(Flags::STRIKEOUT) {
            canvas.fill(x, y + cell_height / 2, width, 1, foreground);
        }
    }
}


fn draw_cursor(canvas: &mut Canvas, shape: CursorShape, x: u32, y: u32, cell: (u32, u32), color: Rgb) {
    let (cell_width, cell_height) = cell;
    match shape {
        CursorShape::Hidden => {}
        CursorShape::Beam => canvas.fill(x, y, 2, cell_height, color),
        CursorShape::Underline => canvas.fill(x, y + cell_height - 2, cell_width, 2, color),
        CursorShape::Block | CursorShape::HollowBlock => {
            canvas.fill(x, y, cell_width, 1, color);
            canvas.fill(x, y + cell_height - 1, cell_width, 1, color);
            canvas.fill(x, y, 1, cell_height, color);
            canvas.fill(x + cell_width - 1, y, 1, cell_height, color);
        }
    }
}


/// Drops every image something has been written over: an image lives on
/// cells left blank for it (`ratatui-image` clears them first, then
/// skips them), so text in any of them means the program has moved on --
/// the F3 preview closed, the panels came back, the screen switched.
fn drop_overwritten_images<T>(term: &Term<T>, images: &mut Vec<PlacedImage>, cell: (u32, u32)) {
    let grid = term.grid();
    let (lines, columns) = (grid.screen_lines(), grid.columns());
    images.retain(|image| {
        let (width, height) = image.cells(cell);
        (image.line..(image.line + height).min(lines)).all(|line| {
            (image.column..(image.column + width).min(columns)).all(|column| {
                let cell = &grid[Line(line as i32)][Column(column)];
                cell.c == ' ' || cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            })
        })
    });
}


/// Where the grid starts in a window of `width` pixels: the part of the
/// width that doesn't fill a whole cell is split between the left and
/// right edges rather than all left over on the right. The leftover
/// height stays at the bottom, under the key bar.
pub fn grid_origin(width: u32, columns: usize, cell_width: u32) -> (u32, u32) {
    (width.saturating_sub(columns as u32 * cell_width) / 2, 0)
}


/// Pixels being drawn into, `0x00RRGGBB` row by row.
struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
    fn fill(&mut self, x: u32, y: u32, width: u32, height: u32, color: Rgb) {
        let packed = pack(color);
        let (left, right) = (x.min(self.width), x.saturating_add(width).min(self.width));
        for row in y..y.saturating_add(height).min(self.height) {
            let start = (row * self.width) as usize;
            self.pixels[start + left as usize..start + right as usize].fill(packed);
        }
    }

    /// A glyph's coverage in `color`, kept within the rows `rows` (its own
    /// grid line), so redrawing one line never leaves a neighbor's glyph
    /// half drawn.
    fn draw_mask(&mut self, x: i32, y: i32, mask: &GlyphMask, color: Rgb, rows: Range<u32>) {
        let packed = pack(color);
        for mask_row in 0..mask.height {
            let pixel_y = y + mask.y + mask_row as i32;
            if pixel_y < rows.start as i32 || pixel_y >= rows.end.min(self.height) as i32 {
                continue;
            }
            let line_start = pixel_y as usize * self.width as usize;
            let coverage_row = &mask.coverage[(mask_row * mask.width) as usize..((mask_row + 1) * mask.width) as usize];
            for (mask_column, &coverage) in coverage_row.iter().enumerate() {
                let pixel_x = x + mask.x + mask_column as i32;
                if coverage == 0 || pixel_x < 0 || pixel_x >= self.width as i32 {
                    continue;
                }
                let pixel = &mut self.pixels[line_start + pixel_x as usize];
                *pixel = if coverage == 255 { packed } else { pack(mix(unpack(*pixel), color, coverage)) };
            }
        }
    }

    fn blit(&mut self, x: u32, y: u32, image: &PlacedImage) {
        for row in 0..image.height.min(self.height.saturating_sub(y)) {
            let line_start = ((y + row) * self.width) as usize;
            for column in 0..image.width.min(self.width.saturating_sub(x)) {
                let index = ((row * image.width + column) * 4) as usize;
                let [r, g, b, alpha] = [image.rgba[index], image.rgba[index + 1], image.rgba[index + 2], image.rgba[index + 3]];
                let pixel = &mut self.pixels[line_start + (x + column) as usize];
                *pixel = pack(mix(unpack(*pixel), Rgb { r, g, b }, alpha));
            }
        }
    }
}


fn pack(color: Rgb) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

fn unpack(pixel: u32) -> Rgb {
    Rgb { r: (pixel >> 16) as u8, g: (pixel >> 8) as u8, b: pixel as u8 }
}

/// `over` on top of `under` at `coverage` (0 = all `under`).
fn mix(under: Rgb, over: Rgb, coverage: u8) -> Rgb {
    let channel = |a: u8, b: u8| ((u16::from(a) * (255 - u16::from(coverage)) + u16::from(b) * u16::from(coverage)) / 255) as u8;
    Rgb { r: channel(under.r, over.r), g: channel(under.g, over.g), b: channel(under.b, over.b) }
}


#[cfg(test)]
mod tests;
