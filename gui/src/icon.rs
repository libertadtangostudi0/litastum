use alacritty_terminal::vte::ansi::Rgb;

use crate::colors::{BACKGROUND, FOREGROUND};

/// The icon's side, in pixels; Windows scales it for the title bar and
/// the taskbar.
pub const SIZE: u32 = 64;

const ACCENT: Rgb = Rgb { r: 0x58, g: 0xa6, b: 0xff };
const CORNER_RADIUS: i32 = 12;

/// The window and taskbar icon, as RGBA rows: two side-by-side panels
/// with a highlighted row in the left one, in the default theme's colors
/// -- drawn here rather than shipped as a file. The exe's own icon in
/// Explorer needs a compiled resource; not done yet.
pub fn rgba() -> Vec<u8> {
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let Some(color) = color_at(x, y) else {
                continue;
            };
            let index = ((y as u32 * SIZE + x as u32) * 4) as usize;
            pixels[index..index + 4].copy_from_slice(&[color.r, color.g, color.b, 0xff]);
        }
    }
    pixels
}


/// The icon's color at `(x, y)`; `None` outside the rounded square.
fn color_at(x: i32, y: i32) -> Option<Rgb> {
    if !inside_rounded_square(x, y) {
        return None;
    }
    let in_rect = |left: i32, top: i32, right: i32, bottom: i32| x >= left && x <= right && y >= top && y <= bottom;
    let outline = |left: i32, top: i32, right: i32, bottom: i32| in_rect(left, top, right, bottom) && !in_rect(left + 2, top + 2, right - 2, bottom - 2);

    if outline(7, 10, 31, 53) || outline(33, 10, 57, 53) || in_rect(11, 18, 27, 22) {
        return Some(ACCENT);
    }
    // A few listed "names" in both panels.
    let text_row = (y - 26) % 6 == 0 || (y - 27) % 6 == 0;
    if (26..=45).contains(&y) && text_row && (in_rect(11, 26, 24, 45) || in_rect(37, 26, 52, 45)) {
        return Some(FOREGROUND);
    }
    Some(BACKGROUND)
}


fn inside_rounded_square(x: i32, y: i32) -> bool {
    let last = SIZE as i32 - 1;
    let corner_x = if x < CORNER_RADIUS { CORNER_RADIUS } else if x > last - CORNER_RADIUS { last - CORNER_RADIUS } else { x };
    let corner_y = if y < CORNER_RADIUS { CORNER_RADIUS } else if y > last - CORNER_RADIUS { last - CORNER_RADIUS } else { y };
    let (dx, dy) = (x - corner_x, y - corner_y);
    dx * dx + dy * dy <= CORNER_RADIUS * CORNER_RADIUS
}


#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
        let index = ((y * SIZE + x) * 4) as usize;
        pixels[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn a_square_rgba_image_with_transparent_rounded_corners() {
        let pixels = rgba();
        assert_eq!(pixels.len(), (SIZE * SIZE * 4) as usize);
        assert_eq!(pixel(&pixels, 0, 0)[3], 0, "outside the rounded corner");
        assert_eq!(pixel(&pixels, SIZE / 2, 2), [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 0xff]);
        assert_eq!(pixel(&pixels, 7, 30), [ACCENT.r, ACCENT.g, ACCENT.b, 0xff], "the left panel's border");
    }
}
