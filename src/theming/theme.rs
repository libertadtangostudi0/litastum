use ratatui::style::Color;


/// Color palette for the whole UI. Values are lifted from the "color 2"
/// GitHub Dark mockup agreed while planning — see
/// `.claude/rules/litastum-ui-theme.md` for the source hex values.
///
/// `Copy` (all fields are `Color`, itself `Copy`): `App` owns the live
/// theme so the F9 menu can swap it at runtime, and rendering copies it
/// out once per frame rather than juggling a borrow of `App` alongside
/// `&mut app.mode` for the whole draw call.
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
    /// Text color to use *over* `current_row_bg`, if a scheme's own
    /// `current_row_bg` is bright/saturated enough that the ordinary
    /// text color (or a file's own type color, in the panel) would read
    /// poorly on it -- `None` (every built-in scheme, and any Windows
    /// Terminal JSON downloaded as-is) means "don't override, keep
    /// whatever color the text would already have," today's behavior
    /// exactly. Requested directly, after setting a theme's own
    /// `selectionBackground` to a vivid ANSI `green` swatch for a bolder
    /// highlight than the theme's own real selection color, and asking
    /// for black text inside it -- reusing the whole-buffer
    /// text or a file's own type color unmodified over a bright green
    /// background would be hard to read. See `ColorScheme::to_theme`/
    /// `ColorScheme::selection_foreground` for the optional,
    /// litastum-specific `selectionForeground` JSON extension this
    /// comes from -- same shape as `command_line_prefix` below, just
    /// with "leave it alone" as the fallback instead of a fixed color,
    /// since forcing a uniform text color over every theme's own
    /// current-row highlight would undo the deliberate "file-type color
    /// survives being the selected row" convention
    /// (`.claude/rules/litastum-theming.md`) for every scheme that
    /// never asked for this.
    pub selection_text: Option<Color>,
    /// The `"{cwd}> "` prefix shown before typed text on the command
    /// line. Defaults to `accent` (and always did, before this field
    /// existed) — kept as its own field rather than folded into
    /// `accent` because a Windows Terminal scheme has no field this can
    /// be derived from in general (real Far Manager uses a distinct,
    /// bold, un-named accent color here — `CommandLine.Prefix` in its
    /// own color scheme — that doesn't correspond to any of the 16
    /// standard ANSI slots a WT scheme defines). See
    /// `ColorScheme::to_theme`/`ColorScheme::command_line_prefix` for
    /// the optional, litastum-specific `commandLinePrefix` JSON
    /// extension this comes from.
    pub command_line_prefix: Color,
    /// Background for a removed line in the Compare view (`Alt+F5`) --
    /// deliberately its own field rather than reusing `danger` directly
    /// as a `.bg()`: `danger` is used everywhere else in this codebase
    /// as a small foreground accent, never a full-line background wash,
    /// and GitHub's own diff colors this view is modeled on are
    /// noticeably dimmer/desaturated than a raw accent-red would look
    /// painted across an entire line against `bg`. Computed once, for
    /// `Theme::dark()`, by hand -- see `diff_added_bg` below for the
    /// same reasoning.
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
            // `danger`/`success` blended ~30% over `bg` by hand (see
            // `blend_over_bg` below for the same computation done at
            // runtime, for a custom scheme) -- a plain `const fn` can't
            // do the floating-point blend itself, so this is the
            // precomputed result, same as `current_row_bg` above.
            // Started at 20%, tried 13% after a report that
            // `github-dark-default` read too burgundy/saturated --
            // reverted the other way after *that* was reported as
            // worse (too pale/washed-out across the themes actually
            // tried) than the original 20%. Landed higher than either,
            // at 30%, per that same follow-up request for a brighter
            // highlight.
            diff_removed_bg: Color::Rgb(0x54, 0x24, 0x26),
            diff_added_bg: Color::Rgb(0x1c, 0x43, 0x28),
        }
    }
}


/// Blends `fg` over `bg` at `alpha` (`0.0` = all `bg`, `1.0` = all
/// `fg`) -- `ratatui` has no real alpha blending (`current_row_bg`'s
/// own doc comment already notes this for the one hand-computed case
/// that predates this function), so this is the general version, used
/// by `ColorScheme::to_theme` to derive `diff_removed_bg`/`diff_added_bg`
/// for a *custom* scheme from its own `danger`/`success` -- `Theme::dark()`'s
/// own values above are this exact computation, done once by hand, at
/// `alpha = 0.3`, since a `const fn` can't do floating-point work in a
/// `const` context.
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
