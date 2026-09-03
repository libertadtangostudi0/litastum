use ratatui::style::Color;


/// Color palette for the whole UI. Values are lifted from the "color 2"
/// GitHub Dark mockup agreed while planning — see
/// `.claude/rules/litastum-ui-theme.md` for the source hex values.
pub struct Theme {
    pub bg: Color,
    pub border: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub danger: Color,
    /// Background for the cursor row in the panel that has keyboard
    /// focus — `accent` blended over `bg` at ~15% alpha (ratatui has no
    /// real alpha blending, so this is precomputed).
    pub current_row_bg: Color,
}


impl Theme {
    /// The (currently only) theme: "color 2", GitHub Dark inspired.
    pub const fn dark() -> Self {
        Self {
            bg: Color::Rgb(0x0d, 0x11, 0x17),
            border: Color::Rgb(0x30, 0x36, 0x3d),
            text: Color::Rgb(0xe6, 0xed, 0xf3),
            text_dim: Color::Rgb(0x8b, 0x94, 0x9e),
            accent: Color::Rgb(0x58, 0xa6, 0xff),
            danger: Color::Rgb(0xf8, 0x51, 0x49),
            current_row_bg: Color::Rgb(0x18, 0x27, 0x3a),
        }
    }
}
