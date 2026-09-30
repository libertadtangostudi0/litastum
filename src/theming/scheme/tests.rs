use super::*;

/// The "Apple System Colors" scheme the user pasted while asking
/// for this feature — also checked into the repo as
/// `themes/apple-system-colors.json`.
const APPLE_SYSTEM_COLORS_JSON: &str = r##"{
    "name": "Apple System Colors",
    "black": "#1a1a1a",
    "red": "#cc372e",
    "green": "#26a439",
    "yellow": "#cdac08",
    "blue": "#0869cb",
    "purple": "#9647bf",
    "cyan": "#479ec2",
    "white": "#98989d",
    "brightBlack": "#464646",
    "brightRed": "#ff453a",
    "brightGreen": "#32d74b",
    "brightYellow": "#ffd60a",
    "brightBlue": "#0a84ff",
    "brightPurple": "#bf5af2",
    "brightCyan": "#76d6ff",
    "brightWhite": "#ffffff",
    "background": "#1e1e1e",
    "foreground": "#ffffff",
    "selectionBackground": "#3f638b",
    "cursorColor": "#98989d"
}"##;

#[test]
fn parses_a_real_windows_terminal_scheme() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    assert_eq!(scheme.name, "Apple System Colors");
    assert_eq!(scheme.background, "#1e1e1e");
    assert_eq!(scheme.bright_purple, "#bf5af2");
}

#[test]
fn rejects_malformed_json() {
    assert!(ColorScheme::from_json_str("{ not json").is_err());
}

#[test]
fn parse_hex_accepts_with_and_without_hash() {
    assert_eq!(parse_hex("#ff0080"), Some((0xff, 0x00, 0x80)));
    assert_eq!(parse_hex("ff0080"), Some((0xff, 0x00, 0x80)));
}

#[test]
fn parse_hex_rejects_bad_input() {
    assert_eq!(parse_hex("#ff00"), None);
    assert_eq!(parse_hex("not-a-color"), None);
}

#[test]
fn to_theme_maps_background_and_foreground() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    let theme = scheme.to_theme();
    assert_eq!(theme.bg, Color::Rgb(0x1e, 0x1e, 0x1e));
    assert_eq!(theme.text, Color::Rgb(0xff, 0xff, 0xff));
    assert_eq!(theme.current_row_bg, Color::Rgb(0x3f, 0x63, 0x8b));
    assert_eq!(theme.danger, Color::Rgb(0xcc, 0x37, 0x2e));
    assert_eq!(theme.warning, Color::Rgb(0xcd, 0xac, 0x08));
    assert_eq!(theme.success, Color::Rgb(0x26, 0xa4, 0x39));
}

#[test]
fn to_theme_accent_uses_cursor_color_when_distinct() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    // cursorColor "#98989d" differs from both background and foreground.
    assert_eq!(scheme.to_theme().accent, Color::Rgb(0x98, 0x98, 0x9d));
}

#[test]
fn to_theme_accent_falls_back_to_blue_when_cursor_color_matches_foreground() {
    let mut scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    scheme.cursor_color = scheme.foreground.clone();
    assert_eq!(scheme.to_theme().accent, rgb(&scheme.blue));
}

/// The `commandLinePrefix` extension is optional -- a real,
/// unmodified Windows Terminal scheme (like the fixture above, which
/// has no such key) must still parse, and `to_theme` should fall
/// back to the same `accent` color it always used before this field
/// existed.
#[test]
fn to_theme_command_line_prefix_falls_back_to_accent_when_absent() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    assert_eq!(scheme.command_line_prefix, None);
    let theme = scheme.to_theme();
    assert_eq!(theme.command_line_prefix, theme.accent);
}

/// When a scheme *does* set the extension (as `far-lts-alien.json`
/// does, to reproduce real Far Manager's own bold terracotta
/// `CommandLine.Prefix` color -- not derivable from any of the 16
/// standard ANSI slots), it should win over the `accent` fallback.
#[test]
fn to_theme_command_line_prefix_uses_the_extension_when_present() {
    let json = r##"{
        "name": "with-prefix",
        "black": "#000000", "red": "#ff0000", "green": "#00ff00", "yellow": "#ffff00",
        "blue": "#0000ff", "purple": "#ff00ff", "cyan": "#00ffff", "white": "#ffffff",
        "brightBlack": "#000000", "brightRed": "#ff0000", "brightGreen": "#00ff00", "brightYellow": "#ffff00",
        "brightBlue": "#0000ff", "brightPurple": "#ff00ff", "brightCyan": "#00ffff", "brightWhite": "#ffffff",
        "background": "#111111", "foreground": "#eeeeee",
        "selectionBackground": "#222222", "cursorColor": "#333333",
        "commandLinePrefix": "#d7875f"
    }"##;
    let scheme = ColorScheme::from_json_str(json).unwrap();
    assert_eq!(scheme.command_line_prefix.as_deref(), Some("#d7875f"));
    assert_eq!(scheme.to_theme().command_line_prefix, Color::Rgb(0xd7, 0x87, 0x5f));
}

/// The `selectionForeground` extension is optional too, same shape
/// as `commandLinePrefix` -- but unlike that one, absence means
/// "don't override anything" (`None`), not a fallback color: forcing
/// a uniform text color over every scheme's own current-row
/// highlight would undo the "file-type color survives being
/// selected" convention for every scheme that never asked for this.
#[test]
fn to_theme_selection_text_is_none_when_selection_foreground_is_absent() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    assert_eq!(scheme.selection_foreground, None);
    assert_eq!(scheme.to_theme().selection_text, None);
}

/// When a scheme *does* set the extension (requested directly, to
/// keep black text readable over a scheme's own selection color
/// deliberately set to a bright ANSI green), it should carry
/// through as `Theme::selection_text`.
#[test]
fn to_theme_selection_text_uses_the_extension_when_present() {
    let json = r##"{
        "name": "with-selection-fg",
        "black": "#000000", "red": "#ff0000", "green": "#00ff00", "yellow": "#ffff00",
        "blue": "#0000ff", "purple": "#ff00ff", "cyan": "#00ffff", "white": "#ffffff",
        "brightBlack": "#000000", "brightRed": "#ff0000", "brightGreen": "#00ff00", "brightYellow": "#ffff00",
        "brightBlue": "#0000ff", "brightPurple": "#ff00ff", "brightCyan": "#00ffff", "brightWhite": "#ffffff",
        "background": "#111111", "foreground": "#eeeeee",
        "selectionBackground": "#98e123", "cursorColor": "#333333",
        "selectionForeground": "#000000"
    }"##;
    let scheme = ColorScheme::from_json_str(json).unwrap();
    assert_eq!(scheme.selection_foreground.as_deref(), Some("#000000"));
    assert_eq!(scheme.to_theme().selection_text, Some(Color::Rgb(0, 0, 0)));
}

#[test]
fn to_syntax_theme_carries_background_and_foreground() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    let syntax_theme = scheme.to_syntax_theme();
    assert_eq!(syntax_theme.settings.background, Some(SynColor { r: 0x1e, g: 0x1e, b: 0x1e, a: 255 }));
    assert_eq!(syntax_theme.settings.foreground, Some(SynColor { r: 0xff, g: 0xff, b: 0xff, a: 255 }));
    assert!(!syntax_theme.scopes.is_empty());
}

/// Resolves the style `syntect` would actually apply to a single
/// TextMate scope under a derived theme — via the real
/// `highlighting::Highlighter`, not just checking our `scopes` list
/// contains a matching entry, so this fails if the selector syntax
/// itself is wrong (e.g. doesn't actually match what it's meant to)
/// as well as if the mapping is missing outright.
fn resolve_style(theme: &SynTheme, scope: &str) -> edtui::syntect::highlighting::Style {
    use std::str::FromStr;

    use edtui::syntect::highlighting::Highlighter;
    use edtui::syntect::parsing::ScopeStack;

    let stack = ScopeStack::from_str(scope).expect("valid scope string");
    Highlighter::new(theme).style_for_stack(stack.as_slice())
}

/// Markdown headings/bold get real styles under a custom scheme -- the
/// grammar ran, but its `markup.*` scopes fell through to plain text.
#[test]
fn to_syntax_theme_gives_markdown_headings_and_bold_a_real_style() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    let syntax_theme = scheme.to_syntax_theme();
    let foreground = syn_rgb(&scheme.foreground);

    let heading_style = resolve_style(&syntax_theme, "markup.heading");
    assert_eq!(heading_style.foreground, syn_rgb(&scheme.cyan));
    assert_ne!(heading_style.foreground, foreground, "should be colored, not plain foreground");
    assert!(heading_style.font_style.contains(FontStyle::BOLD));

    let bold_style = resolve_style(&syntax_theme, "markup.bold");
    assert_eq!(bold_style.foreground, syn_rgb(&scheme.yellow));
    assert!(bold_style.font_style.contains(FontStyle::BOLD));

    let link_style = resolve_style(&syntax_theme, "markup.underline.link");
    assert_eq!(link_style.foreground, syn_rgb(&scheme.blue));
}

/// Diff scopes get colors under a custom scheme (names from
/// sublimehq/Packages' `Diff.sublime-syntax`) -- the same gap as Markdown.
#[test]
fn to_syntax_theme_gives_diff_added_removed_and_changed_lines_a_real_style() {
    let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
    let syntax_theme = scheme.to_syntax_theme();
    let foreground = syn_rgb(&scheme.foreground);

    let inserted_style = resolve_style(&syntax_theme, "markup.inserted.diff");
    assert_eq!(inserted_style.foreground, syn_rgb(&scheme.green));
    assert_ne!(inserted_style.foreground, foreground, "should be colored, not plain foreground");

    let deleted_style = resolve_style(&syntax_theme, "markup.deleted.diff");
    assert_eq!(deleted_style.foreground, syn_rgb(&scheme.red));

    let changed_style = resolve_style(&syntax_theme, "markup.changed.diff");
    assert_eq!(changed_style.foreground, syn_rgb(&scheme.yellow));

    let header_style = resolve_style(&syntax_theme, "meta.diff.header.from-file meta.header.from-file.diff");
    assert_eq!(header_style.foreground, syn_rgb(&scheme.cyan));
    assert!(header_style.font_style.contains(FontStyle::BOLD));

    let range_style = resolve_style(&syntax_theme, "meta.diff.range.unified");
    assert_eq!(range_style.foreground, syn_rgb(&scheme.blue));
}
