use ratatui::style::Color;

use crate::theming::Theme;

/// Hands the theme's text, background and cursor colors to the terminal
/// itself (`OSC 10`/`11`/`12`): what litastum doesn't paint -- most of the
/// screen shows the terminal's own background -- then matches the theme
/// in any terminal, and litastum's own window (`gui/`) also colors its
/// frame from it. `None` for a theme without RGB colors.
pub(crate) fn palette_sequence(theme: &Theme) -> Option<String> {
    let (Color::Rgb(fg_r, fg_g, fg_b), Color::Rgb(bg_r, bg_g, bg_b), Color::Rgb(cursor_r, cursor_g, cursor_b)) = (theme.text, theme.bg, theme.accent) else {
        return None;
    };
    let osc = |number: u8, r: u8, g: u8, b: u8| format!("\x1b]{number};rgb:{r:02x}/{g:02x}/{b:02x}\x1b\\");
    Some(format!("{}{}{}", osc(10, fg_r, fg_g, fg_b), osc(11, bg_r, bg_g, bg_b), osc(12, cursor_r, cursor_g, cursor_b)))
}

/// Gives the terminal its own colors back (`OSC 110`/`111`/`112`), on exit.
pub(crate) const RESET_PALETTE: &str = "\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\";


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_text_background_and_cursor_in_xterm_form() {
        let theme = Theme::dark();
        assert_eq!(
            palette_sequence(&theme).unwrap(),
            "\x1b]10;rgb:e6/ed/f3\x1b\\\x1b]11;rgb:0d/11/17\x1b\\\x1b]12;rgb:58/a6/ff\x1b\\"
        );
    }

    #[test]
    fn a_theme_without_rgb_colors_sets_nothing() {
        let mut theme = Theme::dark();
        theme.bg = Color::Reset;
        assert_eq!(palette_sequence(&theme), None);
    }
}
