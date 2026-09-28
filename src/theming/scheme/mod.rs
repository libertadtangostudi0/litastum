use edtui::syntect::highlighting::{
    Color as SynColor, FontStyle, ScopeSelectors, StyleModifier, Theme as SynTheme, ThemeItem, ThemeSettings,
};
use ratatui::style::Color;
use serde::Deserialize;

use super::theme::{blend_over_bg, Theme};


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
    /// Optional, litastum-specific extension to the standard Windows
    /// Terminal JSON shape — not part of the format WT itself writes,
    /// so absent from every scheme sourced from
    /// <https://windowsterminalthemes.dev/>. Exists because real Far
    /// Manager's own `CommandLine.Prefix` color (bold, a distinct
    /// terracotta/orange in the "far-lts-alien" scheme) doesn't
    /// correspond to any of the 16 standard ANSI slots above — there's
    /// no principled way to derive it from them the way every other
    /// `Theme` field is derived. `#[serde(default)]` so existing scheme
    /// files without it keep parsing exactly as before; `to_theme`
    /// falls back to the same `accent` color when it's absent.
    #[serde(default, rename = "commandLinePrefix")]
    pub command_line_prefix: Option<String>,
    /// Optional, litastum-specific extension, same shape as
    /// `command_line_prefix` above — absent from every scheme sourced
    /// from <https://windowsterminalthemes.dev/>. Overrides the text
    /// color drawn *over* `selectionBackground` (the panel's current
    /// row, the editor's own text selection); `None` (the default for
    /// every scheme that doesn't set this) leaves that text whatever
    /// color it would already be — see `Theme::selection_text`'s own
    /// field doc comment for why "leave it alone" is the fallback here,
    /// unlike `command_line_prefix`'s fallback to `accent`.
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


    /// Derives a `syntect` theme for the editor's syntax highlighting
    /// from the same 16 colors, base16-style — Windows Terminal's 8
    /// base colors line up closely with base16's
    /// keyword/string/comment/etc. roles (that mapping is base16's own
    /// original design, not a stretch we're inventing). Deliberately
    /// modest: common scopes only, not an exhaustive TextMate grammar.
    ///
    /// Includes `markup.*` (Markdown headings/bold/italic/lists/links/
    /// quotes/code) alongside the original code-oriented scopes — found
    /// missing after a report that `.md` files "have no highlighting"
    /// under a custom `editor_theme`: `syntect`'s bundled default set
    /// *does* include a Markdown grammar (unlike `.ps1` — see
    /// `editor.rs`'s `syntect_bundles_rust_but_not_powershell` test),
    /// so the highlighter was genuinely running, but every `markup.*`
    /// scope it emitted fell through to this theme's plain `foreground`
    /// with nothing here naming it — indistinguishable from "no
    /// highlighting" even though it technically wasn't that. The
    /// built-in `dracula` fallback theme (`editor.rs::SYNTAX_THEME`,
    /// used with no `editor_theme` configured) already had real
    /// `markup.*` rules of its own, which is why this gap only showed
    /// up once a *custom* scheme was applied.
    ///
    /// Same exact gap, same fix, hit again for `.diff`/`.patch`: they
    /// have a real grammar (`syntect`'s own bundled default already
    /// resolves `.diff`/`.patch` to a "Diff" `SyntaxReference` —
    /// confirmed directly, no `BUNDLED_GRAMMARS` entry needed), but its
    /// `markup.inserted.diff`/`markup.deleted.diff`/`markup.changed.diff`/
    /// `meta.diff.*` scopes (verified against sublimehq/Packages' own
    /// `Diff/Diff.sublime-syntax`) had nothing here naming them either
    /// — reported as "works when a prior build without a custom
    /// `editor_theme` configured is run, not with a fresh build using
    /// the configured one," which pointed straight at this same
    /// custom-theme-only gap rather than a build issue. `markup.inserted`/
    /// `markup.deleted`/`markup.changed` (no `.diff` suffix) are prefix
    /// selectors -- they also match any *other* grammar using the same
    /// convention (e.g. a unified-diff-shaped `gitcommit`/`patch`
    /// grammar), not just this one.
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
