use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// The 16 ANSI colors before litastum sets any: the bundled default theme
/// (`themes/github-dark-default.json`), so a plain color looks like the
/// rest of the app.
const ANSI: [Rgb; 16] = [
    rgb(0x2f3742),
    rgb(0xff7b72),
    rgb(0x3fb950),
    rgb(0xd29922),
    rgb(0x58a6ff),
    rgb(0xbe8fff),
    rgb(0x39c5cf),
    rgb(0xf0f6fc),
    rgb(0x656c76),
    rgb(0xffa198),
    rgb(0x56d364),
    rgb(0xe3b341),
    rgb(0x79c0ff),
    rgb(0xd2a8ff),
    rgb(0x56d4dd),
    rgb(0xffffff),
];
pub const FOREGROUND: Rgb = rgb(0xc9d1d9);
pub const BACKGROUND: Rgb = rgb(0x0d1117);
const CURSOR: Rgb = rgb(0x58a6ff);

const fn rgb(value: u32) -> Rgb {
    Rgb { r: (value >> 16) as u8, g: (value >> 8) as u8, b: value as u8 }
}


/// A cell color as RGB: what the program set (`colors`, from OSC
/// sequences) wins, then the defaults above, the xterm 256-color cube and
/// the grayscale ramp.
pub fn resolve(color: Color, colors: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Named(named) => colors[named].unwrap_or_else(|| named_default(named)),
        Color::Indexed(index) => colors[index as usize].unwrap_or_else(|| indexed_default(index)),
    }
}


fn named_default(named: NamedColor) -> Rgb {
    match named {
        NamedColor::Foreground | NamedColor::BrightForeground => FOREGROUND,
        NamedColor::Background => BACKGROUND,
        NamedColor::Cursor => CURSOR,
        NamedColor::DimForeground => dim(FOREGROUND),
        other => {
            let index = other as usize;
            if index < 16 {
                ANSI[index]
            } else {
                // The Dim* variants follow the 16 plus the specials; dim
                // the matching normal color.
                dim(ANSI[(index - NamedColor::DimBlack as usize) % 8])
            }
        }
    }
}


fn indexed_default(index: u8) -> Rgb {
    match index {
        0..=15 => ANSI[index as usize],
        16..=231 => {
            let cube = index - 16;
            let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
            Rgb { r: level(cube / 36), g: level(cube / 6 % 6), b: level(cube % 6) }
        }
        _ => {
            let gray = 8 + (index - 232) * 10;
            Rgb { r: gray, g: gray, b: gray }
        }
    }
}


fn dim(color: Rgb) -> Rgb {
    Rgb { r: color.r / 3 * 2, g: color.g / 3 * 2, b: color.b / 3 * 2 }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_256_color_cube_and_ramp_follow_xterm() {
        assert_eq!(indexed_default(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(indexed_default(196), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(indexed_default(231), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(indexed_default(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(indexed_default(255), Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn named_colors_come_from_the_default_theme() {
        assert_eq!(named_default(NamedColor::Red), rgb(0xff7b72));
        assert_eq!(named_default(NamedColor::BrightWhite), rgb(0xffffff));
        assert_eq!(named_default(NamedColor::Background), BACKGROUND);
    }
}
