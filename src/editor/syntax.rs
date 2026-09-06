use std::sync::{Arc, OnceLock};

use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder};
use edtui::{SyntaxHighlighter, SYNTAX_SET, THEME_SET};
use tracing::warn;

/// syntect theme bundled with `edtui`. `edtui`'s own docs are
/// inconsistent about naming here — `SyntaxHighlighter::theme`'s field
/// doc example says `"base16-ocean.dark"` (dot), but its own bundled
/// theme list a few lines below spells the same theme
/// `"base16-ocean-dark"` (hyphen) — the dotted form silently failed to
/// resolve (`SyntaxHighlighter::new` returned `Err`, so highlighting
/// was quietly skipped entirely). Using `"dracula"` instead: spelled
/// the same way everywhere in the crate's docs, so there's no similar
/// trap.
pub(super) const SYNTAX_THEME: &str = "dracula";

/// `.sublime-syntax` (YAML) grammars for extensions `syntect`'s own
/// bundled default set (sourced from sublimehq/Packages) doesn't cover
/// at all — bundled at compile time via `include_str!`, licenses kept
/// alongside each in `assets/syntax/`:
///
/// - PowerShell (`.ps1`/`.psm1`/`.psd1`) — confirmed missing by
///   `syntect_bundles_rust_but_not_powershell` below. `syntect` only
///   loads the YAML `.sublime-syntax` format itself (its `plist-load`
///   feature is for `.tmTheme` *color themes*, not `.tmLanguage`
///   grammars — the obvious first choice, Microsoft's own
///   github.com/PowerShell/EditorSyntax, ships only a `.tmLanguage`
///   and turned out to be a dead end for that reason). This one is
///   from github.com/SublimeText/PowerShell (MIT license).
/// - INI (`.ini`/`.cfg`/`.conf`, and — via its own `hidden_file_extensions`
///   list — `.editorconfig` and a handful of other INI-shaped dotfiles)
///   — sublimehq/Packages has no INI syntax at all (checked directly,
///   not just syntect's build of it), so this isn't a syntect-specific
///   gap either. From github.com/jwortmann/ini-syntax (Apache-2.0
///   license).
/// - TOML (`.toml`, and `Cargo.lock`/`Gopkg.lock`/... via its own
///   `hidden_file_extensions`), Git Ignore (`.gitignore`), and Git
///   Attributes (`.gitattributes`) — all three genuinely present in
///   sublimehq/Packages (confirmed directly, browsing the repo) but,
///   unlike almost everything else there, apparently not included in
///   `syntect`'s own default bundle for some unknown reason. Same
///   permissive license as the rest of that repo
///   (`assets/syntax/sublimehq-Packages.LICENSE.txt`) — the exact
///   source `syntect`'s own default set is already built from, so
///   pulling a few more files from it raises no new licensing question.
///   Git Ignore and Git Attributes both `include:` rules from a shared
///   `Git Common.sublime-syntax` (`hidden: true` — not selectable by
///   extension on its own, only usable as an include target); it has
///   to be in this same `SyntaxSet` too or those includes silently
///   resolve to nothing and the file opens with no highlighting at all
///   (found by hand: `.gitignore` opened fine but rendered with zero
///   color, since a `SyntaxHighlighter` still resolved even when a
///   grammar's own internal includes don't — `syntect` doesn't treat
///   that as a load error).
/// - Git Config (`.gitconfig`/`.gitmodules` by name, and — via its own
///   `first_line_match: ^\[core\]` — plain `.git/config`, which has no
///   usable name or extension of its own at all). This is what pushed
///   `resolve_syntax_highlighter` below to add a first-line lookup
///   tier, not just name/extension: `.git/config` was never going to
///   be reachable any other way, and the grammar itself already
///   assumes that's how it'll be found.
const BUNDLED_GRAMMARS: &[&str] = &[
    include_str!("../../assets/syntax/PowerShell.sublime-syntax"),
    include_str!("../../assets/syntax/INI.sublime-syntax"),
    include_str!("../../assets/syntax/TOML.sublime-syntax"),
    include_str!("../../assets/syntax/GitCommon.sublime-syntax"),
    include_str!("../../assets/syntax/GitIgnore.sublime-syntax"),
    include_str!("../../assets/syntax/GitAttributes.sublime-syntax"),
    include_str!("../../assets/syntax/GitConfig.sublime-syntax"),
];

/// A `SyntaxSet` containing just `BUNDLED_GRAMMARS` (not `edtui`'s own
/// shared default set — `syntect::parsing::SyntaxSet` isn't `Clone`, so
/// there's no cheap way to extend the one `edtui` already loaded;
/// building a second, minimal one just for these is simpler than
/// re-loading the entire default bundle a second time). Parsed once,
/// lazily, only if a file needing one of them is actually opened.
pub(super) fn bundled_extra_syntax_set() -> &'static Arc<SyntaxSet> {
    static SET: OnceLock<Arc<SyntaxSet>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = SyntaxSetBuilder::new();
        for source in BUNDLED_GRAMMARS {
            match SyntaxDefinition::load_from_str(source, true, None) {
                Ok(syntax) => builder.add(syntax),
                Err(err) => warn!(%err, "failed to parse a bundled .sublime-syntax grammar"),
            }
        }
        Arc::new(builder.build())
    })
}

/// Resolves a `SyntaxHighlighter` for a file, general-purpose: tries
/// `candidates` (typically `[file_name, extension]`) against `syntect`'s
/// own bundled set, then our extra `BUNDLED_GRAMMARS`; if neither
/// matched by name at all, falls back to `first_line` against both sets
/// in the same order — matching `syntect`'s own convenience method
/// `SyntaxSet::find_syntax_for_file`'s two-tier lookup (name, then
/// first line), just spread across two `SyntaxSet`s instead of one.
///
/// The first-line tier exists specifically for files with no usable
/// name of their own — `.git/config` has neither a recognizable
/// extension nor (usually) any distinguishing part of its path, but
/// `GitConfig.sublime-syntax` declares `first_line_match: ^\[core\]`
/// for exactly this reason. Without this tier, *any* future grammar
/// that leans on first-line detection (shebang scripts, XML doctypes,
/// ...) would need its own one-off special case instead of just
/// working the way its own grammar file already says it should.
pub(super) fn resolve_syntax_highlighter(candidates: &[&str], first_line: &str, custom_syntax_theme: &Option<SynTheme>) -> Option<SyntaxHighlighter> {
    let extra_set = bundled_extra_syntax_set();

    for candidate in candidates {
        if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_extension(candidate) {
            return build_highlighter(SYNTAX_SET.clone(), syntax_ref.clone(), custom_syntax_theme);
        }
    }
    for candidate in candidates {
        if let Some(syntax_ref) = extra_set.find_syntax_by_extension(candidate) {
            return build_highlighter(extra_set.clone(), syntax_ref.clone(), custom_syntax_theme);
        }
    }

    if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_first_line(first_line) {
        return build_highlighter(SYNTAX_SET.clone(), syntax_ref.clone(), custom_syntax_theme);
    }
    if let Some(syntax_ref) = extra_set.find_syntax_by_first_line(first_line) {
        return build_highlighter(extra_set.clone(), syntax_ref.clone(), custom_syntax_theme);
    }

    None
}

/// Builds a `SyntaxHighlighter` from an already-resolved `syntax_set`/
/// `syntax_ref` pair, applying `custom_syntax_theme` in place of the
/// named `SYNTAX_THEME` fallback if one is set. `None` only if
/// `SYNTAX_THEME` itself somehow isn't in `THEME_SET` (would mean the
/// bundled theme dump is broken, not a per-file lookup failure).
fn build_highlighter(syntax_set: Arc<SyntaxSet>, syntax_ref: SyntaxReference, custom_syntax_theme: &Option<SynTheme>) -> Option<SyntaxHighlighter> {
    let theme_set = THEME_SET.clone();
    let theme = match custom_syntax_theme {
        Some(custom) => custom.clone(),
        None => theme_set.themes.get(SYNTAX_THEME)?.clone(),
    };
    Some(SyntaxHighlighter::with_sets(theme, theme_set, syntax_ref, syntax_set))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;
    use crate::theming::Theme;

    /// Pins down what's actually true about `syntect`'s bundled default
    /// syntax set, found by hand while debugging a "no highlighting for
    /// .ps1" report: `.rs` is bundled, `.ps1` (PowerShell) is not — not
    /// a bug in `Editor::view`'s extension lookup, an upstream gap, and
    /// the reason `custom_extension_highlighter`/`POWERSHELL_SYNTAX`
    /// exist at all. If `syntect` ever adds/drops one of these, this
    /// will fail and flag it rather than silently changing behavior.
    #[test]
    fn syntect_bundles_rust_but_not_powershell() {
        assert!(SyntaxHighlighter::new(SYNTAX_THEME, "rs").is_ok());
        assert!(SyntaxHighlighter::new(SYNTAX_THEME, "ps1").is_err());
    }

    /// The actual fix for the gap above: our own bundled
    /// `.sublime-syntax` grammar covers what `syntect`'s default set
    /// doesn't, for all three PowerShell extensions (case-insensitively,
    /// matching `find_syntax_by_extension`'s own behavior) — and stays
    /// `None` for something neither set has, rather than panicking.
    #[test]
    fn resolve_syntax_highlighter_covers_powershell_extensions() {
        assert!(resolve_syntax_highlighter(&["ps1"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["PS1"], "", &None).is_some(), "should match case-insensitively");
        assert!(resolve_syntax_highlighter(&["psm1"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["psd1"], "", &None).is_some());
        assert!(
            resolve_syntax_highlighter(&["rs"], "", &None).is_some(),
            "should still resolve syntect's own bundled grammars, not just ours"
        );
        assert!(resolve_syntax_highlighter(&["made-up-extension"], "", &None).is_none());
    }

    #[test]
    fn resolve_syntax_highlighter_covers_ini_extensions() {
        assert!(resolve_syntax_highlighter(&["ini"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["INI"], "", &None).is_some(), "should match case-insensitively");
        assert!(resolve_syntax_highlighter(&["cfg"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["conf"], "", &None).is_some());
    }

    /// Not a new grammar of its own — `INI.sublime-syntax`'s own
    /// `hidden_file_extensions` already lists `.editorconfig` (a full
    /// *file name*, not a bare extension), so this comes for free once
    /// the INI grammar was bundled for `.ini`/`.cfg`/`.conf`.
    #[test]
    fn resolve_syntax_highlighter_covers_editorconfig_via_the_ini_grammar() {
        assert!(resolve_syntax_highlighter(&[".editorconfig"], "", &None).is_some());
    }

    #[test]
    fn resolve_syntax_highlighter_covers_toml_and_git_formats() {
        assert!(resolve_syntax_highlighter(&["toml"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["TOML"], "", &None).is_some(), "should match case-insensitively");
        assert!(
            resolve_syntax_highlighter(&["Cargo.lock"], "", &None).is_some(),
            "TOML.sublime-syntax's own hidden_file_extensions covers this by full file name, not extension"
        );
        assert!(resolve_syntax_highlighter(&["gitignore"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&["gitattributes"], "", &None).is_some());
    }

    #[test]
    fn resolve_syntax_highlighter_covers_gitconfig_by_name() {
        assert!(resolve_syntax_highlighter(&["gitconfig"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&[".gitconfig"], "", &None).is_some());
        assert!(resolve_syntax_highlighter(&[".gitmodules"], "", &None).is_some());
    }

    /// The actual point of the first-line lookup tier: `.git/config` has
    /// no usable name of its own — its `file_name` candidate is just
    /// `"config"`, which no grammar declares by name — but
    /// `GitConfig.sublime-syntax`'s own `first_line_match: ^\[core\]`
    /// makes it resolvable anyway once the first line is checked too.
    #[test]
    fn resolve_syntax_highlighter_finds_git_config_by_first_line_when_the_name_is_useless() {
        assert!(
            resolve_syntax_highlighter(&["config"], "", &None).is_none(),
            "sanity: bare 'config' shouldn't match anything by name alone"
        );
        assert!(resolve_syntax_highlighter(&["config"], "[core]", &None).is_some());
    }

    /// Regression test for a real bug found by hand after the fixes
    /// above shipped: `.gitignore` opened without a crash and name-based
    /// lookup returned `Some`, but the file rendered with *zero*
    /// color — Git Ignore's own grammar `include:`s rules from a
    /// separate `Git Common.sublime-syntax` (`hidden: true`) that
    /// hadn't been bundled alongside it, so every `include:` silently
    /// resolved to nothing. `syntect` doesn't treat an unresolved
    /// include as a load error, so a `SyntaxHighlighter` still resolved
    /// regardless — the only way to actually catch this is to run real
    /// highlighting and check it colors *something*, which
    /// `resolve_syntax_highlighter_covers_*` above doesn't do.
    #[test]
    fn gitignore_comments_are_actually_colored_not_just_resolvable() {
        use edtui::syntect::easy::HighlightLines;

        let syntax_set = bundled_extra_syntax_set();
        let syntax_ref = syntax_set.find_syntax_by_extension(".gitignore").expect("gitignore syntax should resolve");
        let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();

        let mut highlighter = HighlightLines::new(syntax_ref, &theme);
        let spans = highlighter.highlight_line("# a comment\n", syntax_set).expect("highlighting should succeed");

        let base_foreground = theme.settings.foreground.expect("theme should define a foreground");
        assert!(
            spans.iter().any(|(style, _)| style.foreground != base_foreground),
            "a comment line should get at least one span colored differently from plain \
             foreground -- if this fails, an `include:` in the bundled grammar isn't resolving \
             again (e.g. a missing shared/common .sublime-syntax dependency)"
        );
    }

    /// `Editor::view`'s actual fallback path, end to end: a bundled-
    /// grammar file gets a working `syntax_highlighter` from
    /// `EditorView`, not just from calling `custom_extension_highlighter`
    /// directly. Includes dotfiles (`.gitignore`/`.gitattributes`) to
    /// pin down the file-name-first lookup fix in `view()` itself —
    /// `Path::extension()` returns `None` for those, so before that fix
    /// they'd never even have reached a highlighter lookup at all, let
    /// alone a successful one.
    #[test]
    fn opening_a_bundled_grammar_file_gets_a_working_syntax_highlighter() {
        use crate::editor::Editor;

        let fixtures = [
            ("script.ps1", "Write-Host 'hi'\n"),
            ("settings.ini", "[section]\nkey=value\n"),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            (".gitignore", "/target\n"),
            (".gitattributes", "* text=auto\n"),
            // No usable name of its own (just "config") -- only
            // resolvable via the first-line lookup tier, see
            // `resolve_syntax_highlighter_finds_git_config_by_first_line_when_the_name_is_useless`.
            ("config", "[core]\n\trepositoryformatversion = 0\n"),
        ];
        for (filename, contents) in fixtures {
            let dir = unique_scratch_dir("editor-bundled");
            let path = dir.join(filename);
            std::fs::write(&path, contents).expect("write test fixture file");
            let mut editor = Editor::open(path, None).expect("open test fixture");

            // EditorView doesn't expose whether a highlighter ended up
            // attached, so this only proves `view()` doesn't panic
            // building one -- `resolve_syntax_highlighter_covers_*`
            // above is what actually pins down that it resolves to `Some`.
            let _ = editor.view(&Theme::dark());
        }
    }
}
