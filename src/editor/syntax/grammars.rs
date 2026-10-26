use std::sync::{Arc, OnceLock};

use edtui::syntect::parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};
use tracing::warn;

/// `.sublime-syntax` (YAML) grammars for files `syntect`'s default set
/// doesn't cover, compiled in; licenses in `assets/syntax/`. Sources:
/// SublimeText/PowerShell, jwortmann/ini-syntax, zyxar/Sublime-CMakeLists,
/// sublimehq/Packages (TOML, Git formats, Groovy); DCL and Visual Studio
/// Solution (`.sln`) are self-authored.
///
/// - A grammar that `include:`s a hidden one needs that one here too
///   (Git Common, CMakeCommands), or it silently colors nothing.
/// - Groovy is patched locally: its `scope:text.html.javadoc` include
///   (no Javadoc grammar here) broke multi-line doc comments.
/// - `.rs` uses `syntect`'s own grammar; a custom one was reverted.
///
/// History: docs/history/theming.md.
const BUNDLED_GRAMMARS: &[&str] = &[
    include_str!("../../../assets/syntax/PowerShell.sublime-syntax"),
    include_str!("../../../assets/syntax/INI.sublime-syntax"),
    include_str!("../../../assets/syntax/TOML.sublime-syntax"),
    include_str!("../../../assets/syntax/GitCommon.sublime-syntax"),
    include_str!("../../../assets/syntax/GitIgnore.sublime-syntax"),
    include_str!("../../../assets/syntax/GitAttributes.sublime-syntax"),
    include_str!("../../../assets/syntax/GitConfig.sublime-syntax"),
    include_str!("../../../assets/syntax/CMakeCommands.sublime-syntax"),
    include_str!("../../../assets/syntax/CMake.sublime-syntax"),
    include_str!("../../../assets/syntax/DCL.sublime-syntax"),
    include_str!("../../../assets/syntax/VisualStudioSolution.sublime-syntax"),
    include_str!("../../../assets/syntax/Groovy.sublime-syntax"),
];

/// Files with no grammar of their own, mapped onto an existing one --
/// matched against the same `[file_name, extension]` candidates, so a
/// full dotfile name works. `.rc` is mostly C preprocessor, so C++ fits;
/// the only public `.rc` grammar is Android's unrelated `init.rc`.
/// `.clang-format`/`.clang-tidy` are real YAML; a solution filter
/// (`.slnf`) is JSON, the newer solution format (`.slnx`) XML.
/// History: docs/history/theming.md.
pub(super) const EXTENSION_ALIASES: &[(&str, &str)] = &[
    ("rc", "cpp"),
    ("rc2", "cpp"),
    (".clang-format", "yaml"),
    (".clang-tidy", "yaml"),
    ("slnf", "json"),
    ("slnx", "xml"),
];

/// Extensions `syntect` claims for a grammar that isn't what this user's
/// files hold, mapped onto the one that is -- tried before `syntect`'s
/// own. `.tmpl` is HTML there; requested as INI (`x.yaml.tmpl` still goes
/// by its inner extension, tried first: `view::syntax_name_candidates`).
pub(super) const EXTENSION_OVERRIDES: &[(&str, &str)] = &[("tmpl", "ini")];

/// A second `SyntaxSet` with just `BUNDLED_GRAMMARS` -- `edtui`'s set
/// can't be extended (`SyntaxSet` isn't `Clone`). Parsed lazily, once.
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
