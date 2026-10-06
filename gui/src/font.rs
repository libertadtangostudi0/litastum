use std::collections::HashMap;
use std::rc::Rc;

use cosmic_text::{Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight};

/// Monospace families tried in order; the first one installed wins.
/// Cascadia Mono ships with Windows 11 and Windows Terminal.
const PREFERRED_FAMILIES: &[&str] = &["Cascadia Mono", "Consolas", "DejaVu Sans Mono", "Menlo", "Liberation Mono"];

/// Line height as a multiple of the font size -- room for box drawing to
/// meet between rows without clipping descenders.
const LINE_HEIGHT_FACTOR: f32 = 1.2;

/// A cell's typeface variant, from the terminal's bold/italic attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub bold: bool,
    pub italic: bool,
}

impl FontStyle {
    pub const REGULAR: Self = Self { bold: false, italic: false };
}


/// One shaped glyph, ready to rasterize at a cell: `x`/`y` place the
/// glyph's own pixels relative to the cell's top-left corner.
#[derive(Clone, Copy)]
struct CellGlyph {
    cache_key: CacheKey,
    x: i32,
    y: i32,
}


/// A rasterized glyph: its coverage (0-255) row by row, `width` wide,
/// placed at `(x, y)` from the cell's top-left corner. Kept per character
/// and style, so drawing a cell is a plain loop over bytes.
pub struct GlyphMask {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
}

/// The terminal font: every character is drawn into one fixed-size cell,
/// so glyphs are shaped one character at a time (once, then cached) and
/// placed by column, never by a shaped line's advances -- a fallback
/// glyph from another font must not push the rest of the row sideways.
pub struct CellFont {
    font_system: FontSystem,
    swash_cache: SwashCache,
    metrics: Metrics,
    family: Option<String>,
    pub cell_width: u32,
    pub cell_height: u32,
    masks: HashMap<(char, FontStyle), Option<Rc<GlyphMask>>>,
}

impl CellFont {
    /// Scans the installed fonts -- slow, so done once per run;
    /// `set_size` reuses the result.
    pub fn new(font_size_px: f32) -> Self {
        let font_system = FontSystem::new();
        let family = installed_family(&font_system);
        let metrics = Metrics::new(font_size_px, 1.0);
        let mut font = Self { font_system, swash_cache: SwashCache::new(), metrics, family, cell_width: 1, cell_height: 1, masks: HashMap::new() };
        font.set_size(font_size_px);
        font
    }

    /// The same fonts at another size (the window's DPI scale); every
    /// cached glyph is redrawn at the new size.
    pub fn set_size(&mut self, font_size_px: f32) {
        self.metrics = Metrics::new(font_size_px, (font_size_px * LINE_HEIGHT_FACTOR).ceil());
        self.masks.clear();
        self.cell_width = self.advance_of('M').round().max(1.0) as u32;
        self.cell_height = self.metrics.line_height.max(1.0) as u32;
    }

    fn attrs(&self, style: FontStyle) -> Attrs<'_> {
        let attrs = match &self.family {
            Some(name) => Attrs::new().family(Family::Name(name)),
            None => Attrs::new().family(Family::Monospace),
        };
        let attrs = if style.bold { attrs.weight(Weight::BOLD) } else { attrs };
        if style.italic {
            attrs.style(Style::Italic)
        } else {
            attrs
        }
    }

    fn shaped(&mut self, c: char, style: FontStyle) -> Option<(f32, CellGlyph)> {
        let mut text = [0u8; 4];
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        let attrs = self.attrs(style).clone();
        buffer.set_text(c.encode_utf8(&mut text), &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let run = buffer.layout_runs().next()?;
        let glyph = run.glyphs.first()?;
        let physical = glyph.physical((0.0, 0.0), 1.0);
        Some((glyph.w, CellGlyph { cache_key: physical.cache_key, x: physical.x, y: run.line_y as i32 + physical.y }))
    }

    fn advance_of(&mut self, c: char) -> f32 {
        self.shaped(c, FontStyle::REGULAR).map_or(self.metrics.font_size * 0.6, |(advance, _)| advance)
    }

    /// `c`'s glyph in `style`, rasterized once and then cached; `None`
    /// for a space or a character no installed font has.
    pub fn mask(&mut self, c: char, style: FontStyle) -> Option<Rc<GlyphMask>> {
        if c == ' ' {
            return None;
        }
        if let Some(mask) = self.masks.get(&(c, style)) {
            return mask.clone();
        }
        let mask = self.rasterized(c, style).map(Rc::new);
        self.masks.insert((c, style), mask.clone());
        mask
    }

    fn rasterized(&mut self, c: char, style: FontStyle) -> Option<GlyphMask> {
        let (_, glyph) = self.shaped(c, style)?;
        let image = self.swash_cache.get_image_uncached(&mut self.font_system, glyph.cache_key)?;
        let placement = image.placement;
        let pixels = (placement.width * placement.height) as usize;
        let coverage = match image.content {
            SwashContent::Mask => image.data,
            // A color glyph (emoji) keeps its shape: alpha as coverage.
            SwashContent::Color | SwashContent::SubpixelMask => image.data.as_chunks::<4>().0.iter().map(|pixel| pixel[3]).collect(),
        };
        if coverage.len() < pixels {
            return None;
        }
        Some(GlyphMask { x: glyph.x + placement.left, y: glyph.y - placement.top, width: placement.width, height: placement.height, coverage })
    }
}


fn installed_family(font_system: &FontSystem) -> Option<String> {
    PREFERRED_FAMILIES
        .iter()
        .find(|wanted| font_system.db().faces().any(|face| face.families.iter().any(|(name, _)| name == *wanted)))
        .map(|name| name.to_string())
}
