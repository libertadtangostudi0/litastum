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

/// Sublime Text has never shipped CMake support by default (it's a
/// third-party package), so this is missing from `syntect`'s own
/// bundle for a different reason than INI/PowerShell above (those
/// are missing because the *upstream source* doesn't have them
/// either; this one exists upstream, just isn't in what Sublime
/// itself ships). Confirmed by hand: a real `CMakeLists.txt` opened
/// with zero color before this grammar was bundled.
#[test]
fn resolve_syntax_highlighter_covers_cmake() {
    assert!(resolve_syntax_highlighter(&["CMakeLists.txt"], "", &None).is_some(), "matched by full file name, not extension");
    assert!(resolve_syntax_highlighter(&["cmake"], "", &None).is_some());
    assert!(resolve_syntax_highlighter(&["CMAKE"], "", &None).is_some(), "should match case-insensitively");
}

/// `CMakeLists.txt` gets the CMake grammar with the real candidate list
/// `["CMakeLists.txt", "txt"]` -- `txt` used to hit Plain Text first.
/// Checks `SyntaxReference::name`, not just that something resolved.
#[test]
fn resolve_syntax_ref_prefers_the_more_specific_candidate_over_a_less_specific_collision() {
    let (_, syntax_ref) = resolve_syntax_ref(&["CMakeLists.txt", "txt"], "").expect("should resolve a grammar");
    assert_eq!(syntax_ref.name, "CMake", "the bare 'txt' extension should not shadow the more specific CMakeLists.txt match");
}

/// A real report: `TBVersionInfo.rc2` (a Windows Resource Script)
/// rendered with zero highlighting -- confirmed `.rc`/`.rc2` have no
/// grammar anywhere (neither `syntect`'s own default set nor our
/// `BUNDLED_GRAMMARS`), unlike CMake above where a grammar existed
/// but lost to a collision. `EXTENSION_ALIASES` borrows C++'s
/// grammar instead, since real `.rc` content is mostly C
/// preprocessor directives.
#[test]
fn rc_files_fall_back_to_the_cpp_grammar() {
    let (_, syntax_ref) = resolve_syntax_ref(&["TBVersionInfo.rc2", "rc2"], "").expect("should resolve via the extension alias");
    assert_eq!(syntax_ref.name, "C++");

    let (_, syntax_ref) = resolve_syntax_ref(&["resource.rc", "rc"], "").expect("should resolve via the extension alias");
    assert_eq!(syntax_ref.name, "C++");
}

/// `.clang-format` has no extension by `Path::extension()`, and no grammar
/// declares the name; it's aliased to YAML, which it is.
#[test]
fn clang_format_files_fall_back_to_the_yaml_grammar() {
    let (_, syntax_ref) = resolve_syntax_ref(&[".clang-format"], "").expect("should resolve via the extension alias");
    assert_eq!(syntax_ref.name, "YAML");

    let (_, syntax_ref) = resolve_syntax_ref(&[".clang-tidy"], "").expect("should resolve via the extension alias");
    assert_eq!(syntax_ref.name, "YAML");
}

/// A real report: this project's own build system names CMake
/// templates `CMakeLists.txt.sdk` (processed into a real
/// `CMakeLists.txt` later) — `resolve_syntax_highlighter` itself
/// only ever sees whatever candidates it's handed, so the actual
/// `.sdk`-stripping happens one layer up in `Editor::view`; this
/// just pins down that the grammar `.sdk`-stripping is meant to
/// reveal is actually there once revealed.
#[test]
fn cmake_grammar_resolves_once_the_sdk_suffix_is_stripped() {
    assert!(resolve_syntax_highlighter(&["CMakeLists.txt.sdk"], "", &None).is_none(), "the raw .sdk-suffixed name alone shouldn't match anything");
    assert!(resolve_syntax_highlighter(&["CMakeLists.txt.sdk", "CMakeLists.txt"], "", &None).is_some(), "but the stripped name, once added as an extra candidate, should");
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

/// `.gitignore` actually gets colored: its grammar `include:`s the hidden
/// Git Common, which must be bundled too -- an unresolved include isn't a
/// load error, so only running real highlighting catches it.
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

/// Same "an unresolved `include:` fails silently" risk as
/// `.gitignore` above, for CMake's own dependency on the separate,
/// `hidden: true` `CMakeCommands.sublime-syntax` (scope
/// `commands.builtin.cmake`, `include:`d from `CMake.sublime-syntax`'s
/// `main` context) — a `set(...)` call should get its command name
/// colored distinctly from plain foreground.
#[test]
fn cmake_commands_include_is_actually_resolved_not_just_present() {
    use edtui::syntect::easy::HighlightLines;

    let syntax_set = bundled_extra_syntax_set();
    let syntax_ref = syntax_set.find_syntax_by_extension("cmake").expect("cmake syntax should resolve");
    let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();

    let mut highlighter = HighlightLines::new(syntax_ref, &theme);
    let spans = highlighter.highlight_line("set(FOO BAR)\n", syntax_set).expect("highlighting should succeed");

    let base_foreground = theme.settings.foreground.expect("theme should define a foreground");
    assert!(
        spans.iter().any(|(style, _)| style.foreground != base_foreground),
        "set(...) should get at least one span colored differently from plain foreground -- \
         if this fails, CMake.sublime-syntax's include of CMakeCommands.sublime-syntax isn't \
         resolving (e.g. CMakeCommands.sublime-syntax missing from BUNDLED_GRAMMARS)"
    );
}

/// Reported missing directly: `.groovy`/`.gradle` and plain
/// `Jenkinsfile` (no extension of its own) had no highlighting at all.
/// `Groovy.sublime-syntax`'s own `hidden_file_extensions` already lists
/// `Jenkinsfile` by full file name, same pattern as `Cargo.lock`/
/// `.editorconfig` elsewhere in this file.
#[test]
fn resolve_syntax_highlighter_covers_groovy_and_jenkinsfile() {
    assert!(resolve_syntax_highlighter(&["groovy"], "", &None).is_some());
    assert!(resolve_syntax_highlighter(&["GROOVY"], "", &None).is_some(), "should match case-insensitively");
    assert!(resolve_syntax_highlighter(&["gvy"], "", &None).is_some());
    assert!(resolve_syntax_highlighter(&["gradle"], "", &None).is_some());
    assert!(
        resolve_syntax_highlighter(&["Jenkinsfile"], "", &None).is_some(),
        "Groovy.sublime-syntax's own hidden_file_extensions covers this by full file name, not extension"
    );
}

/// A single-line Groovy block comment colors at all. The multi-line case
/// below is the one the grammar patch was for.
#[test]
fn groovy_comments_are_actually_colored_not_just_resolvable() {
    use edtui::syntect::easy::HighlightLines;

    let syntax_set = bundled_extra_syntax_set();
    let syntax_ref = syntax_set.find_syntax_by_extension("groovy").expect("groovy syntax should resolve");
    let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();

    let mut highlighter = HighlightLines::new(syntax_ref, &theme);
    let spans = highlighter.highlight_line("/* a comment */\n", syntax_set).expect("highlighting should succeed");

    let base_foreground = theme.settings.foreground.expect("theme should define a foreground");
    assert!(
        spans.iter().any(|(style, _)| style.foreground != base_foreground),
        "a comment line should get at least one span colored differently from plain foreground"
    );
}

/// Every line of a multi-line `/** ... */` in Groovy colors as comment,
/// through one shared `HighlightLines` so parse state carries between
/// lines. The middle lines used to render as code. History: docs/history/theming.md.
#[test]
fn groovy_multiline_doc_comment_colors_every_line_as_comment() {
    use edtui::syntect::easy::HighlightLines;

    let syntax_set = bundled_extra_syntax_set();
    let syntax_ref = syntax_set.find_syntax_by_extension("groovy").expect("groovy syntax should resolve");
    let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();
    let base_foreground = theme.settings.foreground.expect("theme should define a foreground");

    let mut highlighter = HighlightLines::new(syntax_ref, &theme);
    let lines = ["/**\n", " * example\n", " * Something\n", " * else\n", " */\n"];

    for line in lines {
        let spans = highlighter.highlight_line(line, syntax_set).expect("highlighting should succeed");
        assert!(
            spans.iter().all(|(style, text)| text.trim().is_empty() || style.foreground != base_foreground),
            "every non-whitespace span on {line:?} should be colored as comment text, not left at \
             plain foreground -- if this fails, the parser is losing track of being inside the \
             comment block partway through, not just failing to color the opening/closing line"
        );
    }
}

/// A decorative banner comment (lines of `***`), reported still broken
/// after the Javadoc-include removal -- passes here, so such a report
/// means the running binary predates the fix.
#[test]
fn groovy_banner_comment_colors_every_line_as_comment() {
    use edtui::syntect::easy::HighlightLines;

    let syntax_set = bundled_extra_syntax_set();
    let syntax_ref = syntax_set.find_syntax_by_extension("groovy").expect("groovy syntax should resolve");
    let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();
    let base_foreground = theme.settings.foreground.expect("theme should define a foreground");

    let mut highlighter = HighlightLines::new(syntax_ref, &theme);
    let lines = [
        "/***************************************************************************\n",
        "***\n",
        "***\n",
        "***    some text here\n",
        "***\n",
        "*****************************************************************************/\n",
    ];

    for line in lines {
        let spans = highlighter.highlight_line(line, syntax_set).expect("highlighting should succeed");
        assert!(
            spans.iter().all(|(style, text)| text.trim().is_empty() || style.foreground != base_foreground),
            "every non-whitespace span on {line:?} should be colored as comment text"
        );
    }

    // The comment must actually close too -- code right after it should
    // be colored as ordinary code again, not still swallowed as comment.
    let spans = highlighter.highlight_line("class Foo {}\n", syntax_set).expect("highlighting should succeed");
    assert!(
        spans.iter().any(|(style, text)| text.trim() == "class" && style.foreground != base_foreground),
        "\"class\" right after the closing */ should be colored as a keyword, not still comment text"
    );
}

/// `base.dcl` gets the self-authored DCL grammar, and the grammar really
/// highlights something.
#[test]
fn dcl_grammar_resolves_and_actually_colors_a_comment() {
    use edtui::syntect::easy::HighlightLines;

    assert!(resolve_syntax_highlighter(&["base.dcl", "dcl"], "", &None).is_some());

    let syntax_set = bundled_extra_syntax_set();
    let syntax_ref = syntax_set.find_syntax_by_extension("dcl").expect("dcl syntax should resolve");
    let theme = THEME_SET.themes.get(SYNTAX_THEME).expect("dracula theme should be bundled").clone();

    let mut highlighter = HighlightLines::new(syntax_ref, &theme);
    let spans = highlighter.highlight_line("//----- Styles of clusters.\n", syntax_set).expect("highlighting should succeed");

    let base_foreground = theme.settings.foreground.expect("theme should define a foreground");
    assert!(
        spans.iter().any(|(style, _)| style.foreground != base_foreground),
        "a comment line should get at least one span colored differently from plain foreground"
    );
}

/// `Editor::view`'s own path gives bundled-grammar files a working
/// highlighter, dotfiles included (`.gitignore` has no
/// `Path::extension()`).
#[test]
fn opening_a_bundled_grammar_file_gets_a_working_syntax_highlighter() {
    use crate::editor::{Editor, EditorKeymapMode};

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
        ("CMakeLists.txt", "cmake_minimum_required(VERSION 3.9)\n"),
        // This project's own build-system convention -- see
        // `Editor::view`'s `.sdk`-suffix-stripping fallback.
        ("CMakeLists.txt.sdk", "cmake_minimum_required(VERSION 3.9)\n"),
        ("base.dcl", "row : cluster {\n    horizontal_margin = none;\n}\n"),
        ("TBVersionInfo.rc2", "#ifndef _MAC\n#include \"./Include/Common/TBVersion.h\"\n#endif\n"),
        ("build.gradle", "apply plugin: 'java'\n"),
        // No extension of its own -- only resolvable by full file name,
        // same shape as "config" above.
        ("Jenkinsfile", "pipeline {\n    agent any\n}\n"),
    ];
    for (filename, contents) in fixtures {
        let dir = unique_scratch_dir("editor-bundled");
        let path = dir.join(filename);
        std::fs::write(&path, contents).expect("write test fixture file");
        let mut editor = Editor::open(path, None, EditorKeymapMode::Standard).expect("open test fixture");

        // EditorView doesn't expose whether a highlighter ended up
        // attached, so this only proves `view()` doesn't panic
        // building one -- `resolve_syntax_highlighter_covers_*`
        // above is what actually pins down that it resolves to `Some`.
        let _ = editor.view(&Theme::dark(), ratatui::layout::Rect::new(0, 0, 40, 10));
    }
}
