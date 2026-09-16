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
        }
    }
}
