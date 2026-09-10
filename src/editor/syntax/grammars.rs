use std::sync::{Arc, OnceLock};

use edtui::syntect::parsing::{SyntaxDefinition, SyntaxSet, SyntaxSetBuilder};
use tracing::warn;

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
/// - CMake (`CMakeLists.txt` by name, `.cmake` by extension) —
///   reported missing for a real project's `CMakeLists.txt.sdk`
///   template files (see `Editor::view`'s `.sdk`-suffix-stripping
///   fallback for why the `.sdk` part resolves at all). Sublime Text
///   itself has never shipped CMake support out of the box — it's a
///   third-party package, so `syntect`'s own default bundle (built
///   from Sublime's actual shipped `Packages`) doesn't have it either,
///   same underlying "not in sublimehq/Packages at all" situation as
///   INI, just for a different reason than "Sublime doesn't ship one"
///   (INI genuinely has none available; CMake has one, it's just not
///   bundled). From github.com/zyxar/Sublime-CMakeLists (MIT license).
///   Its main grammar `include:`s a second, `hidden: true` file
///   (`CMakeCommands.sublime-syntax`, scope `commands.builtin.cmake`)
///   for command-argument highlighting — same "the include target has
///   to be bundled too, or it silently resolves to nothing" situation
///   as `Git Common.sublime-syntax` above, so both are included here.
/// - AutoCAD Dialog Control Language (`.dcl`) — reported missing for a
///   real project's `base.dcl`. No suitable existing grammar found
///   anywhere (the only public `.dcl` grammar is OpenVMS's unrelated
///   "DIGITAL Command Language", which would mis-highlight rather than
///   not highlight — same trap as Android's `init.rc` for `.rc`, see
///   `EXTENSION_ALIASES` below), so this one is self-authored — no
///   `LICENSE.txt` alongside it, since there's no upstream source to
///   attribute. Deliberately modest (comments, strings, numbers,
///   punctuation, tile-type labels, attribute names), not an
///   exhaustive enumeration of DCL's full tile/attribute vocabulary.
///
/// - Groovy (`.groovy`/`.gvy`/`.gradle`, and — via its own
///   `hidden_file_extensions` list — plain `Jenkinsfile`, which has no
///   extension of its own at all) — reported missing directly. Genuinely
///   present in sublimehq/Packages (confirmed directly, browsing the
///   repo) but, like TOML/the Git formats above, apparently not
///   included in `syntect`'s own default bundle for some unknown
///   reason — same license situation too
///   (`assets/syntax/sublimehq-Packages.LICENSE.txt`), no separate
///   `LICENSE.txt` needed. **One local patch**: the upstream grammar's
///   own `comments` context tried `include: scope:text.html.javadoc`
///   first (for `/** ... */` doc-comment blocks), falling back to a
///   plain `/* ... */` comment-block match — a *cross-scope* reference
///   to an entirely different grammar (Java's own Javadoc) that this
///   project doesn't bundle, unlike the *local*, same-file/`hidden:
///   true`-target includes Git Ignore/CMake depend on above. Reported
///   directly against a real multi-line `/** ... */` block: the opening
///   and closing lines colored fine, but every line in between rendered
///   as ordinary code (`*` as an operator, the following word as a
///   plain identifier) instead of comment text — removed the
///   `scope:text.html.javadoc` line entirely from this project's own
///   copy of the grammar rather than chase the exact interaction
///   further, since it can only ever be a liability here (there's no
///   Javadoc grammar in this `SyntaxSet` for it to ever successfully
///   resolve against) and the plain `comment-block` fallback is
///   correct and sufficient on its own — see
///   `groovy_multiline_doc_comment_colors_every_line_as_comment`.
///
/// A custom Rust grammar (github.com/rust-lang/rust-enhanced) was tried
/// here too, to get closer to VS Code's own highlighting -- reverted:
/// even after two rounds of local patches (widening its type coverage,
/// then unifying primitive vs. named types onto one scope), it still
/// didn't hold up against real-world comparison and was pulled rather
/// than chase it further. `.rs` is back to `syntect`'s own bundled
/// grammar, same as before that attempt -- see
/// `syntect_bundles_rust_but_not_powershell` below for confirmation
/// it's genuinely there.
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
    include_str!("../../../assets/syntax/Groovy.sublime-syntax"),
];

/// Extensions with no dedicated grammar anywhere (bundled here or in
/// `syntect`'s own default set) that should still get *some* real
/// highlighting rather than none, by borrowing an existing grammar
/// close enough to be useful — Windows Resource Script
/// (`.rc`/`.rc2`, e.g. `TBVersionInfo.rc2`) reported as unhighlighted:
/// its content is overwhelmingly C preprocessor directives
/// (`#include`/`#ifdef`/`#define`, as in the actual report) plus a
/// handful of RC-specific keywords (`DIALOGEX`, `BEGIN`/`END`, control
/// types), so C++'s grammar (already in `syntect`'s own bundled set)
/// gets real, useful highlighting for most of a typical `.rc` file's
/// content. No dedicated `.rc` grammar was bundled instead because the
/// only public one found (`google/sublime-text-android-syntax`'s
/// `init.rc.sublime-syntax`) is Android's *init.rc* — an unrelated
/// language that happens to share the extension, and would mis-
/// highlight rather than not highlight.
pub(super) const EXTENSION_ALIASES: &[(&str, &str)] = &[("rc", "cpp"), ("rc2", "cpp")];

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
