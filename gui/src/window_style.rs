use alacritty_terminal::vte::ansi::Rgb;
use winit::window::{Icon, Theme, Window, WindowAttributes};

use crate::{colors, icon};

/// A dark window frame in the terminal's own colors, whatever the system
/// theme: the title bar was the system's light or accent color (yellow
/// in the report) above a dark grid. The exact colors are Windows 11's;
/// elsewhere the dark theme is as far as it goes. Also the icon.
pub fn themed(attributes: WindowAttributes) -> WindowAttributes {
    let icon = Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE).ok();
    let attributes = attributes.with_theme(Some(Theme::Dark)).with_window_icon(icon.clone());
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::{Color, WindowAttributesExtWindows};
        let frame_color = |rgb: Rgb| Color::from_rgb(rgb.r, rgb.g, rgb.b);
        attributes
            .with_taskbar_icon(icon)
            .with_title_background_color(Some(frame_color(colors::BACKGROUND)))
            .with_title_text_color(frame_color(colors::FOREGROUND))
            .with_border_color(Some(frame_color(colors::BACKGROUND)))
    };
    attributes
}


/// The title bar, its text and the border in `background`/`text` -- the
/// program's colors, which litastum hands over with `OSC 10`/`11` when its
/// theme changes. Windows 11 only.
pub fn set_frame_colors(window: &Window, background: Rgb, text: Rgb) {
    #[cfg(windows)]
    {
        use winit::platform::windows::{Color, WindowExtWindows};
        window.set_title_background_color(Some(Color::from_rgb(background.r, background.g, background.b)));
        window.set_title_text_color(Color::from_rgb(text.r, text.g, text.b));
        window.set_border_color(Some(Color::from_rgb(background.r, background.g, background.b)));
    }
    #[cfg(not(windows))]
    let _ = (window, background, text);
}
