use edtui::syntect::highlighting::{
    Color as SynColor, ScopeSelectors, StyleModifier, Theme as SynTheme, ThemeItem, ThemeSettings,
};
use ratatui::style::Color;
use serde::Deserialize;

use crate::theme::Theme;


/// A Windows Terminal-format color scheme — the exact JSON shape
/// Windows Terminal itself uses for `colorSchemes`, and what
/// <https://windowsterminalthemes.dev/> exports, so a downloaded file
/// drops in unmodified. See `.claude/rules/litastum-theming.md` for the
/// mapping this drives onto our own `Theme` and the editor's syntax
/// theme, and the reasoning behind it.
///
/// `black`/`white`/most `bright*` fields aren't consumed by
/// `to_theme`/`to_syntax_theme` yet (see the mapping table in
/// `.claude/rules/litastum-theming.md`) — kept anyway, rather than
/// dropped, for full round-trip fidelity with the WT JSON format and
/// for future mapping expansion; `#[allow(dead_code)]` documents that
/// as deliberate instead of silencing a real oversight.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ColorScheme {
    #[serde(default)]
    pub name: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub purple: String,
    pub cyan: String,
    pub white: String,
    #[serde(rename = "brightBlack")]
    pub bright_black: String,
    #[serde(rename = "brightRed")]
    pub bright_red: String,
    #[serde(rename = "brightGreen")]
    pub bright_green: String,
    #[serde(rename = "brightYellow")]
    pub bright_yellow: String,
    #[serde(rename = "brightBlue")]
    pub bright_blue: String,
    #[serde(rename = "brightPurple")]
    pub bright_purple: String,
    #[serde(rename = "brightCyan")]
    pub bright_cyan: String,
    #[serde(rename = "brightWhite")]
    pub bright_white: String,
    pub background: String,
    pub foreground: String,
    #[serde(rename = "selectionBackground")]
    pub selection_background: String,
    #[serde(rename = "cursorColor")]
    pub cursor_color: String,
}


impl ColorScheme {
    /// Parses a Windows Terminal color scheme from its JSON text.
    pub fn from_json_str(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }


    /// Maps this scheme onto our own UI chrome palette. Windows
    /// Terminal schemes have no concept of "accent" or "border" — see
    /// `.claude/rules/litastum-theming.md` for the mapping convention
    /// used here and the reasoning behind each choice.
    pub fn to_theme(&self) -> Theme {
        let accent = if hex_eq(&self.cursor_color, &self.background) || hex_eq(&self.cursor_color, &self.foreground) {
            rgb(&self.blue)
        } else {
            rgb(&self.cursor_color)
        };

        Theme {
            bg: rgb(&self.background),
            border: rgb(&self.bright_black),
            text: rgb(&self.foreground),
            text_dim: rgb(&self.bright_black),
            accent,
            danger: rgb(&self.red),
            warning: rgb(&self.yellow),
            success: rgb(&self.green),
            current_row_bg: rgb(&self.selection_background),
        }
    }


    /// Derives a `syntect` theme for the editor's syntax highlighting
    /// from the same 16 colors, base16-style — Windows Terminal's 8
    /// base colors line up closely with base16's
    /// keyword/string/comment/etc. roles (that mapping is base16's own
    /// original design, not a stretch we're inventing). Deliberately
    /// modest: common scopes only, not an exhaustive TextMate grammar.
    pub fn to_syntax_theme(&self) -> SynTheme {
        let settings = ThemeSettings {
            background: Some(syn_rgb(&self.background)),
            foreground: Some(syn_rgb(&self.foreground)),
            selection: Some(syn_rgb(&self.selection_background)),
            caret: Some(syn_rgb(&self.cursor_color)),
            ..ThemeSettings::default()
        };

        let scopes = vec![
            scope_item("keyword, storage", &self.purple),
            scope_item("string", &self.green),
            scope_item("comment", &self.bright_black),
            scope_item("constant.numeric, constant.language", &self.yellow),
            scope_item("entity.name.function, support.function", &self.blue),
            scope_item("entity.name.type, entity.name.class, support.type", &self.cyan),
            scope_item("variable.parameter, entity.name.tag", &self.red),
        ];

        SynTheme {
            name: Some(self.name.clone()),
            author: None,
            settings,
            scopes,
        }
    }
}


/// Builds one syntax-theme scope rule. `selector` is a hardcoded,
/// known-valid TextMate scope selector (never user input), so a parse
/// failure here would mean a typo in this file, not bad theme data.
fn scope_item(selector: &str, hex: &str) -> ThemeItem {
    ThemeItem {
        scope: selector.parse::<ScopeSelectors>().expect("hardcoded scope selector must be valid"),
        style: StyleModifier {
            foreground: Some(syn_rgb(hex)),
            background: None,
            font_style: None,
        },
    }
}


/// Parses a `"#rrggbb"` (or `"rrggbb"`) hex string. `None` on anything
/// else — malformed theme files degrade to black for that one color
/// rather than failing the whole theme.
fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}


fn rgb(hex: &str) -> Color {
    let (r, g, b) = parse_hex(hex).unwrap_or((0, 0, 0));
    Color::Rgb(r, g, b)
}


fn syn_rgb(hex: &str) -> SynColor {
    let (r, g, b) = parse_hex(hex).unwrap_or((0, 0, 0));
    SynColor { r, g, b, a: 255 }
}


/// Case-insensitive hex string comparison (some scheme files mix
/// `#FFFFFF`/`#ffffff` casing; a literal string compare would treat
/// those as different colors).
fn hex_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}


#[cfg(test)]
mod tests {
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

    #[test]
    fn to_syntax_theme_carries_background_and_foreground() {
        let scheme = ColorScheme::from_json_str(APPLE_SYSTEM_COLORS_JSON).unwrap();
        let syntax_theme = scheme.to_syntax_theme();
        assert_eq!(syntax_theme.settings.background, Some(SynColor { r: 0x1e, g: 0x1e, b: 0x1e, a: 255 }));
        assert_eq!(syntax_theme.settings.foreground, Some(SynColor { r: 0xff, g: 0xff, b: 0xff, a: 255 }));
        assert!(!syntax_theme.scopes.is_empty());
    }
}
