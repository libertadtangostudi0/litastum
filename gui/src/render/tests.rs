use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::Processor;

use super::*;
use crate::session::GridSize;

fn term(columns: usize, lines: usize) -> Term<VoidListener> {
    Term::new(Config::default(), &GridSize { columns, lines }, VoidListener)
}

fn print(term: &mut Term<VoidListener>, bytes: &[u8]) {
    let mut parser: Processor = Processor::new();
    parser.advance(term, bytes);
}

fn view_for(font: &CellFont, columns: u32, lines: u32) -> View {
    View { width: columns * font.cell_width, height: lines * font.cell_height, origin: (0, 0), cursor_visible: false }
}

/// The pixels of one cell, top-left first.
fn cell_pixels(pixels: &[u32], view: View, font: &CellFont, column: u32, line: u32) -> Vec<u32> {
    (0..font.cell_height)
        .flat_map(|y| (0..font.cell_width).map(move |x| ((line * font.cell_height + y) * view.width + column * font.cell_width + x) as usize))
        .map(|index| pixels[index])
        .collect()
}


#[test]
fn mixing_runs_from_the_background_to_the_glyph_color() {
    let (black, white) = (Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 255, g: 255, b: 255 });
    assert_eq!(mix(black, white, 0), black);
    assert_eq!(mix(black, white, 255), white);
    assert_eq!(mix(black, white, 128).r, 128);
}

/// The real terminal engine fed a red "A": its cell gets glyph pixels in
/// the ANSI red, an empty cell only the background.
#[test]
fn draws_what_the_program_printed() {
    let mut term = term(4, 2);
    print(&mut term, b"\x1b[31mA");
    let mut font = CellFont::new(16.0);
    let view = view_for(&font, 4, 2);
    let red = pack(colors::resolve(Color::Named(NamedColor::Red), term.colors()));

    let pixels = Renderer::new().draw(&mut term, &mut font, &mut Vec::new(), view).to_vec();

    assert!(cell_pixels(&pixels, view, &font, 0, 0).contains(&red), "the A is drawn in red");
    assert!(cell_pixels(&pixels, view, &font, 3, 1).iter().all(|pixel| *pixel == pack(colors::BACKGROUND)), "an empty cell is background");
}

/// litastum hands its theme's background over with `OSC 11`; the cells
/// it doesn't paint, and the margins, follow it.
#[test]
fn the_programs_background_color_fills_unpainted_cells_and_margins() {
    let mut term = term(2, 1);
    print(&mut term, b"\x1b]11;rgb:12/34/56\x1b\\");
    let mut font = CellFont::new(16.0);
    let view = View { width: 2 * font.cell_width + 6, height: font.cell_height, origin: (3, 0), cursor_visible: false };

    let pixels = Renderer::new().draw(&mut term, &mut font, &mut Vec::new(), view).to_vec();

    assert_eq!(pixels[0], 0x123456, "the margin");
    assert_eq!(pixels[(3 + font.cell_width + 2) as usize], 0x123456, "an unpainted cell");
}

/// Reported: the side margins were uneven, all of the leftover width
/// sitting on the right.
#[test]
fn the_leftover_width_is_split_between_both_sides() {
    assert_eq!(grid_origin(1000, 90, 11), (5, 0), "1000 - 990 = 10 pixels, 5 a side");
    assert_eq!(grid_origin(990, 90, 11), (0, 0));
    assert_eq!(grid_origin(100, 90, 11), (0, 0), "a grid wider than the window starts at the edge");
}

/// The point of the renderer: a change on one line redraws that line and
/// leaves the rest of the back buffer alone -- every cell and glyph used
/// to be redrawn each frame.
#[test]
fn only_changed_lines_are_redrawn() {
    let mut term = term(4, 3);
    let mut font = CellFont::new(16.0);
    let view = view_for(&font, 4, 3);
    let mut renderer = Renderer::new();
    renderer.draw(&mut term, &mut font, &mut Vec::new(), view);
    // A marker no draw would produce, on the untouched last line.
    let marker_index = (2 * font.cell_height * view.width + 1) as usize;
    renderer.back[marker_index] = 0xabcdef;

    print(&mut term, b"x");
    let pixels = renderer.draw(&mut term, &mut font, &mut Vec::new(), view).to_vec();

    assert_eq!(pixels[marker_index], 0xabcdef, "line 2 wasn't redrawn");
    let foreground = pack(colors::FOREGROUND);
    assert!(cell_pixels(&pixels, view, &font, 0, 0).contains(&foreground), "line 0 shows the new x");
}

#[test]
fn invalidate_redraws_everything() {
    let mut term = term(4, 3);
    let mut font = CellFont::new(16.0);
    let view = view_for(&font, 4, 3);
    let mut renderer = Renderer::new();
    renderer.draw(&mut term, &mut font, &mut Vec::new(), view);
    let marker_index = (2 * font.cell_height * view.width + 1) as usize;
    renderer.back[marker_index] = 0xabcdef;

    renderer.invalidate();
    let pixels = renderer.draw(&mut term, &mut font, &mut Vec::new(), view).to_vec();

    assert_eq!(pixels[marker_index], pack(colors::BACKGROUND));
}

/// The cursor moving away must not leave its bar behind on the old line,
/// though nothing in the terminal changed there.
#[test]
fn the_cursors_old_line_is_redrawn_when_it_moves() {
    let mut term = term(4, 3);
    print(&mut term, b"\x1b[5 q");
    let mut font = CellFont::new(16.0);
    let view = View { cursor_visible: true, ..view_for(&font, 4, 3) };
    let cursor = pack(colors::resolve(Color::Named(NamedColor::Cursor), term.colors()));
    let mut renderer = Renderer::new();
    let first = renderer.draw(&mut term, &mut font, &mut Vec::new(), view).to_vec();
    assert!(cell_pixels(&first, view, &font, 0, 0).contains(&cursor), "the bar on line 0");

    print(&mut term, b"\x1b[3;1H");
    let pixels = renderer.draw(&mut term, &mut font, &mut Vec::new(), view).to_vec();

    assert!(!cell_pixels(&pixels, view, &font, 0, 0).contains(&cursor), "gone from line 0");
    assert!(cell_pixels(&pixels, view, &font, 0, 2).contains(&cursor), "now on line 2");
}

fn image_at(line: usize, column: usize, width: u32, height: u32) -> PlacedImage {
    PlacedImage { line, column, width, height, rgba: [0, 255, 0, 255].repeat((width * height) as usize) }
}

#[test]
fn an_image_is_drawn_over_its_cells_and_stays_while_they_are_blank() {
    let mut term = term(4, 2);
    let mut font = CellFont::new(16.0);
    let view = view_for(&font, 4, 2);
    let mut images = vec![image_at(1, 1, 3, 2)];
    let mut renderer = Renderer::new();

    let pixels = renderer.draw(&mut term, &mut font, &mut images, view).to_vec();
    assert_eq!(pixels[(font.cell_height * view.width + font.cell_width) as usize], 0x00ff00);

    renderer.draw(&mut term, &mut font, &mut images, view);
    assert_eq!(images.len(), 1, "its cells are blank, so it stays");
}

/// The F3 preview closing: text drawn into an image's cells ends it, and
/// its pixels are gone from the next frame.
#[test]
fn an_image_goes_once_text_is_written_over_it() {
    let mut term = term(4, 2);
    let mut font = CellFont::new(16.0);
    let view = view_for(&font, 4, 2);
    let mut images = vec![image_at(1, 1, 3, 2), image_at(0, 0, 1, 1)];
    let mut renderer = Renderer::new();
    renderer.draw(&mut term, &mut font, &mut images, view);

    print(&mut term, b"\x1b[2;2Hx");
    let pixels = renderer.draw(&mut term, &mut font, &mut images, view).to_vec();

    assert_eq!(images.len(), 1);
    assert_eq!((images[0].line, images[0].column), (0, 0), "the image elsewhere stays");
    let where_the_image_was = (font.cell_height * view.width + font.cell_width + 1) as usize;
    assert_ne!(pixels[where_the_image_was], 0x00ff00);
}

#[test]
fn fill_clips_at_the_canvas_edge() {
    let mut pixels = vec![0u32; 4 * 2];
    let mut canvas = Canvas { pixels: &mut pixels, width: 4, height: 2 };
    canvas.fill(3, 1, 5, 5, Rgb { r: 1, g: 2, b: 3 });
    assert_eq!(pixels, [0, 0, 0, 0, 0, 0, 0, 0x010203]);
}

/// A glyph taller than its cell is cut at the cell's line, so a line
/// redrawn alone never paints into its neighbors.
#[test]
fn a_glyph_stays_within_its_own_line() {
    let mut pixels = vec![0u32; 2 * 4];
    let mut canvas = Canvas { pixels: &mut pixels, width: 2, height: 4 };
    let mask = GlyphMask { x: 0, y: -1, width: 1, height: 4, coverage: vec![255; 4] };
    canvas.draw_mask(0, 1, &mask, Rgb { r: 0, g: 0, b: 9 }, 1..3);
    assert_eq!(pixels, [0, 0, 9, 0, 9, 0, 0, 0], "rows 1 and 2 only");
}
