use base64::Engine;

/// An inline image placed on the grid, its top-left corner at a cell.
pub struct PlacedImage {
    pub line: usize,
    pub column: usize,
    pub width: u32,
    pub height: u32,
    /// RGBA, row by row.
    pub rgba: Vec<u8>,
}


/// Decodes an iTerm2 inline image body (`inline=1;size=..;width=..px;...:BASE64`
/// -- what `ratatui-image` sends, a PNG) into an image placed at
/// `(line, column)`, the cursor's cell when it arrived. `None` for
/// anything not shown inline or that doesn't decode.
pub fn decode(body: &[u8], line: usize, column: usize) -> Option<PlacedImage> {
    let separator = body.iter().position(|&byte| byte == b':')?;
    let arguments = std::str::from_utf8(&body[..separator]).ok()?;
    if !arguments.split(';').any(|argument| argument == "inline=1") {
        return None;
    }
    let encoded: Vec<u8> = body[separator + 1..].iter().copied().filter(|byte| !byte.is_ascii_whitespace()).collect();
    let data = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let image = image::load_from_memory(&data).ok()?.to_rgba8();
    Some(PlacedImage { line, column, width: image.width(), height: image.height(), rgba: image.into_raw() })
}


impl PlacedImage {
    /// The cells the image covers, at `cell` pixels per cell.
    pub fn cells(&self, cell: (u32, u32)) -> (usize, usize) {
        (self.width.div_ceil(cell.0.max(1)) as usize, self.height.div_ceil(cell.1.max(1)) as usize)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn png_2x1() -> Vec<u8> {
        let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        let mut png = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        png
    }

    #[test]
    fn decodes_what_ratatui_image_sends() {
        let payload = base64::engine::general_purpose::STANDARD.encode(png_2x1());
        let body = format!("inline=1;size=10;width=2px;height=1px;doNotMoveCursor=1:{payload}");

        let image = decode(body.as_bytes(), 3, 4).unwrap();

        assert_eq!((image.line, image.column, image.width, image.height), (3, 4, 2, 1));
        assert_eq!(image.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn a_download_rather_than_an_inline_image_is_ignored() {
        let payload = base64::engine::general_purpose::STANDARD.encode(png_2x1());
        assert!(decode(format!("name=eA==:{payload}").as_bytes(), 0, 0).is_none());
    }

    #[test]
    fn covered_cells_round_up() {
        let image = PlacedImage { line: 0, column: 0, width: 25, height: 40, rgba: Vec::new() };
        assert_eq!(image.cells((10, 20)), (3, 2));
    }
}
