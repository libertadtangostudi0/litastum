use edtui::syntect::highlighting::{
    Color as SynColor, FontStyle, ScopeSelectors, StyleModifier, Theme as SynTheme, ThemeItem, ThemeSettings,
};
use ratatui::style::Color;
use serde::Deserialize;

use super::theme::{blend_over_bg, Theme};


/// A Windows Terminal color scheme, in WT's own `colorSchemes` JSON (what
/// <https://windowsterminalthemes.dev/> exports). Mapping:
/// `.claude/rules/litastum-theming.md`. Fields not mapped yet are kept for
/// round-trip fidelity; `#[allow(dead_code)]` marks that as deliberate.
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
    /// litastum-specific, optional: Far's bold `CommandLine.Prefix` color,
    /// which no ANSI slot holds. Absent -> `accent`.
    #[serde(default, rename = "commandLinePrefix")]
    pub command_line_prefix: Option<String>,
    /// litastum-specific, optional: text color over `selectionBackground`.
    /// Absent -> leave the text color alone (see `Theme::selection_text`).
    #[serde(default, rename = "selectionForeground")]
    pub selection_foreground: Option<String>,
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

        let command_line_prefix = self.command_line_prefix.as_deref().map(rgb).unwrap_or(accent);
        let bg = rgb(&self.background);
        let danger = rgb(&self.red);
        let success = rgb(&self.green);

        Theme {
            bg,
            border: rgb(&self.bright_black),
            text: rgb(&self.foreground),
            text_dim: rgb(&self.bright_black),
            accent,
            danger,
            warning: rgb(&self.yellow),
            success,
            current_row_bg: rgb(&self.selection_background),
            selection_text: self.selection_foreground.as_deref().map(rgb),
            command_line_prefix,
            // Same 30% blend `Theme::dark()` uses, computed here at
            // runtime instead of by hand -- see `theme::blend_over_bg`'s
            // own doc comment.
            diff_removed_bg: blend_over_bg(danger, bg, 0.3),
            diff_added_bg: blend_over_bg(success, bg, 0.3),
        }
    }


    /// Four representative colors for the F9 color-scheme picker's
    /// swatch preview (`ui/theme_menu.rs`) -- `red`/`yellow`/`green`/
    /// `cyan`, a fixed, arbitrary pick (not curated per scheme) meant
    /// to give a quick "warm to cool" sense of a scheme's palette at a
    /// glance, same spirit as the theming doc's own "decided
    /// unilaterally, easy to revisit" mapping choices.
    pub fn preview_swatch(&self) -> [Color; 4] {
        [rgb(&self.red), rgb(&self.yellow), rgb(&self.green), rgb(&self.cyan)]
    }


    /// Derives the editor's `syntect` theme from the 16 colors, base16-style
    /// (WT's base colors map onto base16's roles). Common scopes only, plus
    /// `markup.*` (Markdown) and the diff scopes -- both rendered plain under
    /// a custom scheme until named here, while the `dracula` fallback colored
    /// them. The un-suffixed diff selectors are prefix matches on purpose.
    /// History: docs/history/theming.md.
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
            // Markdown (and other markup-language) scopes -- picked to
            // stay visually distinct from the code scopes above, not
            // copied from any particular reference theme.
            bold_scope_item("markup.heading", &self.cyan),
            bold_scope_item("markup.bold", &self.yellow),
            italic_scope_item("markup.italic", &self.purple),
            scope_item("markup.list, punctuation.definition.list_item", &self.red),
            italic_scope_item("markup.quote", &self.bright_black),
            scope_item("markup.underline.link, markup.link, string.other.link", &self.blue),
            scope_item("markup.raw, markup.raw.inline, markup.raw.block", &self.green),
            scope_item(
                "punctuation.definition.heading, punctuation.definition.bold, \
                 punctuation.definition.italic, punctuation.definition.link",
                &self.bright_black,
            ),
            // Diff/patch scopes (`Diff/Diff.sublime-syntax` in
            // sublimehq/Packages, `syntect`'s own default bundle) --
            // added/removed/changed lines, file headers, hunk ranges.
            scope_item("markup.inserted", &self.green),
            scope_item("markup.deleted", &self.red),
            scope_item("markup.changed", &self.yellow),
            bold_scope_item("meta.diff.header, meta.header", &self.cyan),
            scope_item("meta.diff.range, punctuation.definition.range", &self.blue),
            scope_item("meta.separator.diff", &self.bright_black),
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
    scope_item_with_style(selector, hex, None)
}


/// A scope rule rendered bold — `markup.heading`/`markup.bold`, so
/// headings and bold text actually look bold in the editor, not just
/// differently colored.
fn bold_scope_item(selector: &str, hex: &str) -> ThemeItem {
    scope_item_with_style(selector, hex, Some(FontStyle::BOLD))
}


/// A scope rule rendered italic — `markup.italic`/`markup.quote`.
fn italic_scope_item(selector: &str, hex: &str) -> ThemeItem {
    scope_item_with_style(selector, hex, Some(FontStyle::ITALIC))
}


fn scope_item_with_style(selector: &str, hex: &str, font_style: Option<FontStyle>) -> ThemeItem {
    ThemeItem {
        scope: selector.parse::<ScopeSelectors>().expect("hardcoded scope selector must be valid"),
        style: StyleModifier {
            foreground: Some(syn_rgb(hex)),
            background: None,
            font_style,
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
mod tests;
