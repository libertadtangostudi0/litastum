use std::collections::HashMap;

use alacritty_terminal::vte::ansi::Rgb;

use super::{pack, Canvas};
use crate::font::{CellFont, FontStyle};

/// Everything that decides how one cell looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub c: char,
    pub style: FontStyle,
    pub foreground: u32,
    pub background: u32,
    pub underline: bool,
    pub strikeout: bool,
    pub hidden: bool,
    /// A wide character's cell: two columns.
    pub wide: bool,
}

impl TileKey {
    pub fn new(c: char, style: FontStyle, foreground: Rgb, background: Rgb) -> Self {
        Self { c, style, foreground: pack(foreground), background: pack(background), underline: false, strikeout: false, hidden: false, wide: false }
    }
}


/// Past this many, the cache starts over -- a screen uses a few hundred.
const MAX_TILES: usize = 8192;


/// Finished cells, one per look (`TileKey`), drawn once and then copied:
/// a full redraw -- scrolling the editor changes every line -- is then
/// memory copies rather than a background fill and a glyph blend per
/// cell. That redraw blocked the window's thread for ~100 ms in a debug
/// build, so held arrow keys queued up, the text jumped, and scrolling
/// ran on after the key was let go. A glyph overhanging its cell
/// (italics) is cut at the cell's edge.
#[derive(Default)]
pub struct TileCache {
    cell: (u32, u32),
    tiles: HashMap<TileKey, Vec<u32>>,
}

impl TileCache {
    /// The pixels of the cell `key` describes, `width` by the cell height.
    pub fn tile(&mut self, key: TileKey, font: &mut CellFont) -> (&[u32], u32) {
        let cell = (font.cell_width, font.cell_height);
        if cell != self.cell || self.tiles.len() >= MAX_TILES {
            self.tiles.clear();
            self.cell = cell;
        }
        let width = if key.wide { cell.0 * 2 } else { cell.0 };
        let tile = self.tiles.entry(key).or_insert_with(|| draw_tile(key, font, width));
        (tile, width)
    }
}


fn draw_tile(key: TileKey, font: &mut CellFont, width: u32) -> Vec<u32> {
    let height = font.cell_height;
    let mut pixels = vec![key.background; (width * height) as usize];
    if key.hidden {
        return pixels;
    }
    let foreground = super::unpack(key.foreground);
    let mut canvas = Canvas { pixels: &mut pixels, width, height };
    if let Some(mask) = font.mask(key.c, key.style) {
        canvas.draw_mask(0, 0, &mask, foreground, 0..height);
    }
    if key.underline {
        canvas.fill(0, height - 2, width, 1, foreground);
    }
    if key.strikeout {
        canvas.fill(0, height / 2, width, 1, foreground);
    }
    pixels
}


#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgb = Rgb { r: 255, g: 255, b: 255 };
    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    #[test]
    fn a_tile_is_drawn_once_and_reused() {
        let mut font = CellFont::new(16.0);
        let mut cache = TileCache::default();
        let key = TileKey::new('A', FontStyle::REGULAR, WHITE, BLACK);

        let first = cache.tile(key, &mut font).0.to_vec();
        let (second, width) = cache.tile(key, &mut font);

        assert_eq!(first, second);
        assert_eq!(width, font.cell_width);
        assert_eq!(cache.tiles.len(), 1);
        assert!(first.contains(&0xffffff), "the glyph");
        assert!(first.contains(&0), "the background");
    }

    #[test]
    fn decorations_and_wide_cells() {
        let mut font = CellFont::new(16.0);
        let mut cache = TileCache::default();
        let underlined = TileKey { underline: true, wide: true, ..TileKey::new(' ', FontStyle::REGULAR, WHITE, BLACK) };

        let (tile, width) = cache.tile(underlined, &mut font);

        assert_eq!(width, font.cell_width * 2);
        let underline_row = ((font.cell_height - 2) * width) as usize;
        assert!(tile[underline_row..underline_row + width as usize].iter().all(|pixel| *pixel == 0xffffff));
    }

    #[test]
    fn a_new_cell_size_starts_the_cache_over() {
        let mut font = CellFont::new(16.0);
        let mut cache = TileCache::default();
        cache.tile(TileKey::new('A', FontStyle::REGULAR, WHITE, BLACK), &mut font);

        font.set_size(30.0);
        let (tile, width) = cache.tile(TileKey::new('B', FontStyle::REGULAR, WHITE, BLACK), &mut font);

        assert_eq!(tile.len(), (width * font.cell_height) as usize);
        assert_eq!(cache.tiles.len(), 1, "the old size's tile is gone");
    }
}
