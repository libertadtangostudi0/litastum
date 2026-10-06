use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::colors;
use crate::font::{CellFont, FontStyle};

/// A window's pixels, `0x00RRGGBB` per pixel, row by row. `origin` is
/// where the grid's top-left cell starts (`grid_origin`).
pub struct Frame<'a> {
    pub pixels: &'a mut [u32],
    pub width: u32,
    pub height: u32,
    pub origin: (u32, u32),
    /// Off during the dark half of a blink (`App::cursor_blink`).
    pub cursor_visible: bool,
}

impl Frame<'_> {
    fn fill(&mut self, x: u32, y: u32, width: u32, height: u32, color: Rgb) {
        let packed = pack(color);
        for row in y..(y + height).min(self.height) {
            let start = (row * self.width + x.min(self.width)) as usize;
            let end = (row * self.width + (x + width).min(self.width)) as usize;
            self.pixels[start..end].fill(packed);
        }
    }

    fn blend(&mut self, x: i32, y: i32, color: Rgb, coverage: u8) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let index = (y as u32 * self.width + x as u32) as usize;
        self.pixels[index] = pack(mix(unpack(self.pixels[index]), color, coverage));
    }
}


/// Draws the whole visible grid, then the cursor. Every cell is drawn
/// every frame; the grid is small enough that the glyph cache, not damage
/// tracking, is what keeps this cheap.
pub fn draw<T: EventListener>(term: &Term<T>, font: &mut CellFont, frame: &mut Frame) {
    let (cell_width, cell_height) = (font.cell_width, font.cell_height);
    let content = term.renderable_content();
    let colors = content.colors;
    // The margins take the program's background (`OSC 11`), like the
    // cells it leaves unpainted.
    frame.fill(0, 0, frame.width, frame.height, colors::resolve(Color::Named(NamedColor::Background), colors));

    for indexed in content.display_iter {
        let line = indexed.point.line.0 + content.display_offset as i32;
        if line < 0 {
            continue;
        }
        let cell = indexed.cell;
        let (x, y) = (frame.origin.0 + indexed.point.column.0 as u32 * cell_width, frame.origin.1 + line as u32 * cell_height);
        let mut foreground = colors::resolve(cell.fg, colors);
        let mut background = colors::resolve(cell.bg, colors);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        if cell.flags.contains(Flags::DIM) {
            foreground = mix(background, foreground, 0xaa);
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { cell_width * 2 } else { cell_width };
        frame.fill(x, y, width, cell_height, background);
        if cell.flags.intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let style = FontStyle { bold: cell.flags.intersects(Flags::BOLD), italic: cell.flags.intersects(Flags::ITALIC) };
        font.rasterize(cell.c, style, |glyph_x, glyph_y, coverage| frame.blend(x as i32 + glyph_x, y as i32 + glyph_y, foreground, coverage));
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            frame.fill(x, y + cell_height - 2, width, 1, foreground);
        }
        if cell.flags.contains(Flags::STRIKEOUT) {
            frame.fill(x, y + cell_height / 2, width, 1, foreground);
        }
    }

    if !frame.cursor_visible {
        return;
    }

    let cursor = content.cursor;
    let line = cursor.point.line.0 + content.display_offset as i32;
    if line < 0 {
        return;
    }
    let (x, y) = (frame.origin.0 + cursor.point.column.0 as u32 * cell_width, frame.origin.1 + line as u32 * cell_height);
    let color = colors::resolve(Color::Named(NamedColor::Cursor), colors);
    match cursor.shape {
        CursorShape::Hidden => {}
        CursorShape::Beam => frame.fill(x, y, 2, cell_height, color),
        CursorShape::Underline => frame.fill(x, y + cell_height - 2, cell_width, 2, color),
        CursorShape::Block | CursorShape::HollowBlock => {
            frame.fill(x, y, cell_width, 1, color);
            frame.fill(x, y + cell_height - 1, cell_width, 1, color);
            frame.fill(x, y, 1, cell_height, color);
            frame.fill(x + cell_width - 1, y, 1, cell_height, color);
        }
    }
}


/// Where the grid starts in a window of `width` pixels: the part of the
/// width that doesn't fill a whole cell is split between the left and
/// right edges rather than all left over on the right. The leftover
/// height stays at the bottom, under the key bar.
pub fn grid_origin(width: u32, columns: usize, cell_width: u32) -> (u32, u32) {
    (width.saturating_sub(columns as u32 * cell_width) / 2, 0)
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
mod tests {
    use super::*;

    #[test]
    fn mixing_runs_from_the_background_to_the_glyph_color() {
        let (black, white) = (Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(mix(black, white, 0), black);
        assert_eq!(mix(black, white, 255), white);
        assert_eq!(mix(black, white, 128).r, 128);
    }

    /// The real terminal engine fed a red "A": its cell gets the default
    /// background and some glyph pixels in the ANSI red.
    #[test]
    fn draws_what_the_program_printed() {
        use alacritty_terminal::event::VoidListener;
        use alacritty_terminal::term::Config;
        use alacritty_terminal::vte::ansi::Processor;

        use crate::session::GridSize;

        let mut term = Term::new(Config::default(), &GridSize { columns: 4, lines: 2 }, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, b"[31mA");
        let mut font = CellFont::new(16.0);
        let (width, height) = (4 * font.cell_width, 2 * font.cell_height);
        let mut pixels = vec![0u32; (width * height) as usize];

        draw(&term, &mut font, &mut Frame { pixels: &mut pixels, width, height, origin: (0, 0), cursor_visible: true });

        let red = pack(colors::resolve(Color::Named(NamedColor::Red), term.colors()));
        let first_cell = |pixel: &u32| *pixel == red;
        let cell_rows = (0..font.cell_height).flat_map(|y| (0..font.cell_width).map(move |x| (y * width + x) as usize));
        assert!(cell_rows.clone().any(|index| first_cell(&pixels[index])), "the A is drawn in red");
        assert_eq!(pixels[(font.cell_height * width + 3 * font.cell_width + 1) as usize], pack(colors::BACKGROUND), "an empty cell is background");
    }

    /// Reported: the side margins were uneven, all of the leftover width
    /// sitting on the right.
    #[test]
    fn the_leftover_width_is_split_between_both_sides() {
        assert_eq!(grid_origin(1000, 90, 11), (5, 0), "1000 - 990 = 10 pixels, 5 a side");
        assert_eq!(grid_origin(990, 90, 11), (0, 0));
        assert_eq!(grid_origin(100, 90, 11), (0, 0), "a grid wider than the window starts at the edge");
    }

    /// litastum hands its theme's background over with `OSC 11`; the
    /// cells it doesn't paint, and the margins, follow it.
    #[test]
    fn the_programs_background_color_fills_unpainted_cells() {
        use alacritty_terminal::event::VoidListener;
        use alacritty_terminal::term::Config;
        use alacritty_terminal::vte::ansi::Processor;

        use crate::session::GridSize;

        let mut term = Term::new(Config::default(), &GridSize { columns: 2, lines: 1 }, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, b"\x1b]11;rgb:12/34/56\x1b\\");
        let mut font = CellFont::new(16.0);
        let (width, height) = (2 * font.cell_width + 6, font.cell_height);
        let mut pixels = vec![0u32; (width * height) as usize];

        draw(&term, &mut font, &mut Frame { pixels: &mut pixels, width, height, origin: (3, 0), cursor_visible: false });

        assert_eq!(pixels[0], 0x123456, "the margin");
        assert_eq!(pixels[(3 + font.cell_width + 2) as usize], 0x123456, "an unpainted cell");
    }

    #[test]
    fn fill_clips_at_the_frame_edge() {
        let mut pixels = vec![0u32; 4 * 2];
        let mut frame = Frame { pixels: &mut pixels, width: 4, height: 2, origin: (0, 0), cursor_visible: true };
        frame.fill(3, 1, 5, 5, Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(pixels, [0, 0, 0, 0, 0, 0, 0, 0x010203]);
    }
}
