use std::sync::Arc;

use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::syntect::parsing::{SyntaxReference, SyntaxSet};
use edtui::{SyntaxHighlighter, SYNTAX_SET, THEME_SET};

mod grammars;

use grammars::{bundled_extra_syntax_set, EXTENSION_ALIASES};

/// The fallback syntect theme bundled with `edtui`. Not
/// `"base16-ocean.dark"`: `edtui`'s docs spell it both with a dot and a
/// hyphen, and the dotted form silently failed to resolve, disabling
/// highlighting. "dracula" is spelled one way everywhere.
pub(super) const SYNTAX_THEME: &str = "dracula";

/// Resolves `(SyntaxSet, SyntaxReference)` for a file without building the
/// highlighter, so tests can see which grammar won (`name`). Order:
/// 1. each of `candidates` (`[file_name, extension]`) against `syntect`'s
///    set *and* `BUNDLED_GRAMMARS` before the next, less specific one --
///    otherwise `CMakeLists.txt`'s `txt` matched Plain Text before the
///    bundled CMake grammar was tried;
/// 2. `EXTENSION_ALIASES`;
/// 3. `first_line` against both sets (`syntect`'s own name-then-first-line
///    lookup, over two sets), so grammars like Git Config
///    (`first_line_match: ^\[core\]`) work without special cases.
///
/// History: docs/history/theming.md.
fn resolve_syntax_ref(candidates: &[&str], first_line: &str) -> Option<(Arc<SyntaxSet>, SyntaxReference)> {
    let extra_set = bundled_extra_syntax_set();

    for candidate in candidates {
        if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_extension(candidate) {
            return Some((SYNTAX_SET.clone(), syntax_ref.clone()));
        }
        if let Some(syntax_ref) = extra_set.find_syntax_by_extension(candidate) {
            return Some((extra_set.clone(), syntax_ref.clone()));
        }
    }

    // Last resort before giving up on names entirely: an extension with
    // no grammar of its own, but a documented close-enough stand-in --
    // see `EXTENSION_ALIASES`.
    for candidate in candidates {
        if let Some((_, aliased)) = EXTENSION_ALIASES.iter().find(|(ext, _)| ext.eq_ignore_ascii_case(candidate)) {
            if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_extension(aliased) {
                return Some((SYNTAX_SET.clone(), syntax_ref.clone()));
            }
        }
    }

    if let Some(syntax_ref) = SYNTAX_SET.find_syntax_by_first_line(first_line) {
        return Some((SYNTAX_SET.clone(), syntax_ref.clone()));
    }
    if let Some(syntax_ref) = extra_set.find_syntax_by_first_line(first_line) {
        return Some((extra_set.clone(), syntax_ref.clone()));
    }

    None
}


/// Resolves a `SyntaxHighlighter` for a file — see `resolve_syntax_ref`
/// for the actual lookup strategy; this just builds the highlighter
/// from whatever it finds.
pub(super) fn resolve_syntax_highlighter(candidates: &[&str], first_line: &str, custom_syntax_theme: &Option<SynTheme>) -> Option<SyntaxHighlighter> {
    if let Some((syntax_set, syntax_ref)) = resolve_syntax_ref(candidates, first_line) {
        return build_highlighter(syntax_set, syntax_ref, custom_syntax_theme);
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
mod tests;
