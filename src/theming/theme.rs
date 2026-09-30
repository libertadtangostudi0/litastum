use ratatui::style::Color;


/// The UI palette; `dark()` holds the "color 2" GitHub Dark values
/// (`.claude/rules/litastum-ui-theme.md`). `Copy`, so drawing copies it
/// once per frame instead of borrowing `App` next to `&mut app.mode`.
#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub border: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub danger: Color,
    /// Archives, and anything else worth flagging without it being an
    /// error — added alongside `HighlightRole::Archive` file coloring
    /// (`panel.rs`). Part of the original "color 2" plan
    /// (`.claude/rules/litastum-ui-theme.md`'s success/danger/warning
    /// triad) that only `danger` actually made it into this struct
    /// initially.
    pub warning: Color,
    /// Executables/scripts (`HighlightRole::Executable`) and anything
    /// else worth marking as a positive/active state. Same
    /// success/danger/warning triad as `warning`, above.
    pub success: Color,
    /// Background for the cursor row in the panel that has keyboard
    /// focus — `accent` blended over `bg` at ~15% alpha (ratatui has no
    /// real alpha blending, so this is precomputed).
    pub current_row_bg: Color,
    /// Text color over `current_row_bg`, for a scheme whose selection color is
    /// bright enough to need one (from `selectionForeground`). `None` keeps
    /// whatever color the text already has -- forcing one would undo file-type
    /// colors on the selected row for every other scheme.
    pub selection_text: Option<Color>,
    /// The command line's `"{cwd}> "` prefix. Its own field because Far's
    /// `CommandLine.Prefix` color matches no WT slot (from
    /// `commandLinePrefix`, else `accent`).
    pub command_line_prefix: Color,
    /// Background of a removed line in Compare: its own field, since `danger`
    /// is a small foreground accent and a full-line wash needs a dimmer color,
    /// like GitHub's. Precomputed for `dark()`; see `diff_added_bg`.
    pub diff_removed_bg: Color,
    /// Background for an added line in the Compare view -- the other
    /// half of `diff_removed_bg` above, same reasoning, dimmed from
    /// `success` instead of `danger`.
    pub diff_added_bg: Color,
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
            warning: Color::Rgb(0xd2, 0x99, 0x22),
            success: Color::Rgb(0x3f, 0xb9, 0x50),
            current_row_bg: Color::Rgb(0x18, 0x27, 0x3a),
            selection_text: None,
            command_line_prefix: Color::Rgb(0x58, 0xa6, 0xff),
            // `danger`/`success` blended 30% over `bg`, precomputed (`blend_over_bg`
            // can't run in a `const fn`). 20% first, then 13% (too burgundy), then
            // back up past 20% on request (13% was washed out). History: docs/history/theming.md.
            diff_removed_bg: Color::Rgb(0x54, 0x24, 0x26),
            diff_added_bg: Color::Rgb(0x1c, 0x43, 0x28),
        }
    }
}


/// Blends `fg` over `bg` at `alpha` (`0.0` all `bg`, `1.0` all `fg`) --
/// `ratatui` has no alpha. Derives the diff backgrounds for custom
/// schemes; `dark()`'s are this at 0.3, done by hand.
pub fn blend_over_bg(fg: Color, bg: Color, alpha: f32) -> Color {
    let (Color::Rgb(fr, fg_, fb), Color::Rgb(br, bg_, bb)) = (fg, bg) else {
        return fg;
    };
    let mix = |f: u8, b: u8| -> u8 { (f as f32 * alpha + b as f32 * (1.0 - alpha)).round() as u8 };
    Color::Rgb(mix(fr, br), mix(fg_, bg_), mix(fb, bb))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_over_bg_at_zero_alpha_is_just_bg() {
        assert_eq!(blend_over_bg(Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 0), 0.0), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn blend_over_bg_at_full_alpha_is_just_fg() {
        assert_eq!(blend_over_bg(Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 0), 1.0), Color::Rgb(255, 0, 0));
    }

    #[test]
    fn blend_over_bg_matches_the_hand_computed_dark_theme_values() {
        // Confirms Theme::dark()'s own hand-computed diff_removed_bg/
        // diff_added_bg literals are actually what this function would
        // produce at alpha = 0.3 -- if either value ever changes, this
        // test should change deliberately, not silently drift apart.
        let theme = Theme::dark();
        assert_eq!(blend_over_bg(theme.danger, theme.bg, 0.3), theme.diff_removed_bg);
        assert_eq!(blend_over_bg(theme.success, theme.bg, 0.3), theme.diff_added_bg);
    }
}
