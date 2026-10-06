use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, CacheKey, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

/// Monospace families tried in order; the first one installed wins.
/// Cascadia Mono ships with Windows 11 and Windows Terminal.
const PREFERRED_FAMILIES: &[&str] = &["Cascadia Mono", "Consolas", "DejaVu Sans Mono", "Menlo", "Liberation Mono"];

/// Line height as a multiple of the font size -- room for box drawing to
/// meet between rows without clipping descenders.
const LINE_HEIGHT_FACTOR: f32 = 1.2;

/// One shaped glyph, ready to rasterize at a cell: `x`/`y` place the
/// glyph's own pixels relative to the cell's top-left corner.
#[derive(Clone, Copy)]
struct CellGlyph {
    cache_key: CacheKey,
    x: i32,
    y: i32,
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
    glyphs: HashMap<char, Option<CellGlyph>>,
}

impl CellFont {
    /// Scans the installed fonts -- slow, so done once per run;
    /// `with_size` reuses the result.
    pub fn new(font_size_px: f32) -> Self {
        Self::build(FontSystem::new(), font_size_px)
    }

    /// The same fonts at another size (the window's DPI scale).
    pub fn with_size(self, font_size_px: f32) -> Self {
        Self::build(self.font_system, font_size_px)
    }

    fn build(font_system: FontSystem, font_size_px: f32) -> Self {
        let family = installed_family(&font_system);
        let metrics = Metrics::new(font_size_px, (font_size_px * LINE_HEIGHT_FACTOR).ceil());
        let mut font = Self { font_system, swash_cache: SwashCache::new(), metrics, family, cell_width: 1, cell_height: 1, glyphs: HashMap::new() };
        font.cell_width = font.advance_of('M').round().max(1.0) as u32;
        font.cell_height = metrics.line_height.max(1.0) as u32;
        font
    }

    fn attrs(&self) -> Attrs<'_> {
        match &self.family {
            Some(name) => Attrs::new().family(Family::Name(name)),
            None => Attrs::new().family(Family::Monospace),
        }
    }

    fn shaped(&mut self, c: char) -> Option<(f32, CellGlyph)> {
        let mut text = [0u8; 4];
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        let attrs = self.attrs().clone();
        buffer.set_text(c.encode_utf8(&mut text), &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let run = buffer.layout_runs().next()?;
        let glyph = run.glyphs.first()?;
        let physical = glyph.physical((0.0, 0.0), 1.0);
        Some((glyph.w, CellGlyph { cache_key: physical.cache_key, x: physical.x, y: run.line_y as i32 + physical.y }))
    }

    fn advance_of(&mut self, c: char) -> f32 {
        self.shaped(c).map_or(self.metrics.font_size * 0.6, |(advance, _)| advance)
    }

    /// Calls `put(x, y, coverage)` for every pixel of `c`'s glyph, `x`/`y`
    /// relative to the cell's top-left corner. Nothing for a space or a
    /// character no installed font has.
    pub fn rasterize(&mut self, c: char, mut put: impl FnMut(i32, i32, u8)) {
        if c == ' ' {
            return;
        }
        let glyph = match self.glyphs.get(&c) {
            Some(glyph) => *glyph,
            None => {
                let glyph = self.shaped(c).map(|(_, glyph)| glyph);
                self.glyphs.insert(c, glyph);
                glyph
            }
        };
        let Some(glyph) = glyph else {
            return;
        };
        self.swash_cache.with_pixels(&mut self.font_system, glyph.cache_key, Color::rgb(0xff, 0xff, 0xff), |x, y, color| {
            put(glyph.x + x, glyph.y + y, color.a());
        });
    }
}


fn installed_family(font_system: &FontSystem) -> Option<String> {
    PREFERRED_FAMILIES
        .iter()
        .find(|wanted| font_system.db().faces().any(|face| face.families.iter().any(|(name, _)| name == *wanted)))
        .map(|name| name.to_string())
}
