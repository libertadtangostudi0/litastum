use std::sync::Arc;

use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::syntect::parsing::{SyntaxReference, SyntaxSet};
use edtui::{SyntaxHighlighter, SYNTAX_SET, THEME_SET};

mod grammars;

use grammars::{bundled_extra_syntax_set, EXTENSION_ALIASES};

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

/// Resolves the `(SyntaxSet, SyntaxReference)` pair for a file, without
/// building the highlighter itself — split out from
/// `resolve_syntax_highlighter` so tests can inspect *which* grammar
/// actually won (via `SyntaxReference`'s public `name` field) instead
/// of only whether a `SyntaxHighlighter` was constructed at all.
///
/// Tries `candidates` (typically `[file_name, extension]`) against
/// `syntect`'s own bundled set and our extra `BUNDLED_GRAMMARS`
/// (`grammars.rs`) **per candidate** — both sets are checked for a
/// given candidate before moving on to the next, less-specific one.
/// This ordering matters: a real bug had this the other way around
/// (all candidates against `SYNTAX_SET`, only then all candidates
/// against `extra_set`), which meant `CMakeLists.txt`'s own literal
/// `.txt` "extension" (from `Path::extension()`, the second, less-
/// specific candidate) matched `syntect`'s ubiquitous bundled Plain
/// Text grammar before the first, more-specific candidate
/// (`"CMakeLists.txt"` itself) ever got a chance to be tried against
/// `extra_set`, where the real CMake grammar lives — so CMake files
/// silently rendered as plain text.
///
/// (A custom Rust grammar briefly needed `extra_set` to win outright
/// over `SYNTAX_SET` for the same extension, and this function's
/// per-candidate order was flipped for that — reverted along with that
/// grammar; every entry in `BUNDLED_GRAMMARS` again exists only for
/// extensions `syntect`'s own set has no grammar for at all, so which
/// set is checked first never actually matters.)
///
/// If no candidate matched a real grammar by name, tries
/// `EXTENSION_ALIASES` next (an extension with no grammar of its own
/// borrowing an existing one close enough to be useful, e.g. `.rc`/
/// `.rc2` → C++). Only then falls back to `first_line` against both
/// sets — matching `syntect`'s own convenience method
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
