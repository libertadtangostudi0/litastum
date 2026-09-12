use std::path::PathBuf;

use super::prompts::parse_far_placeholder;

/// One panel's worth of state for macro substitution (`substitute_macros`) --
/// built fresh from `App`/`Panel` right before running a menu item's
/// commands (`input::run_selected_user_menu_item`'s own
/// `build_macro_context`), never stored. Lives here (rather than
/// alongside `Panel` itself) so this module -- the one place that
/// actually needs to reason about Far's `!##`/`!^`/`![`/`!]` panel-
/// context prefixes -- stays the single source of truth for what a
/// macro substitution needs to know, without pulling `crate::app`/
/// `crate::explorer::panel` into this otherwise pure, I/O-free module.
#[derive(Debug, Clone, Default)]
pub struct PanelMacroContext {
    pub dir: PathBuf,
    /// The file under the cursor, if any (`Panel::selected_path`) --
    /// `None` on an empty panel or with the cursor on `..`.
    pub cursor: Option<PathBuf>,
    /// Marked files, or just the cursor file if none are marked --
    /// same "marked wins over cursor" rule `Panel::marked_or_current`
    /// already uses for F5/F6/F8.
    pub selected: Vec<PathBuf>,
}

/// The four panel contexts a Far macro can resolve against -- `active`/
/// `passive` follow whichever panel currently has focus (`app.active`),
/// `left`/`right` are the fixed on-screen panels regardless of focus,
/// matching real Far Manager's own four-way addressing
/// (`!^`/`!##`/`![`/`!]`).
#[derive(Debug, Clone, Default)]
pub struct MacroContext {
    pub active: PanelMacroContext,
    pub passive: PanelMacroContext,
    pub left: PanelMacroContext,
    pub right: PanelMacroContext,
}

impl MacroContext {
    fn panel(&self, which: PanelRef) -> &PanelMacroContext {
        match which {
            PanelRef::Active => &self.active,
            PanelRef::Passive => &self.passive,
            PanelRef::Left => &self.left,
            PanelRef::Right => &self.right,
        }
    }
}

impl PanelMacroContext {
    fn file_name_with_ext(&self) -> String {
        self.cursor.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
    fn file_stem(&self) -> String {
        self.cursor.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
    fn extension(&self) -> String {
        self.cursor.as_ref().and_then(|p| p.extension()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
    fn dir_display(&self) -> String {
        self.dir.display().to_string()
    }
    /// Real Far's `!:` is the current drive letter (`"C:"`) or a UNC
    /// share root -- approximated here as the path's own first
    /// component, which is exactly that on Windows (a `Prefix`
    /// component's `OsStr` renders as `"C:"`/`"\\\\server\\share"`) and
    /// merely "the first path segment" elsewhere, where there's no
    /// drive-letter concept to begin with.
    fn drive_display(&self) -> String {
        self.dir.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default()
    }
    fn inline_list(&self, quote: bool) -> String {
        self.selected
            .iter()
            .map(|p| {
                let s = p.display().to_string();
                if quote { format!("\"{s}\"") } else { s }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}


/// Which panel a Far macro currently resolves against -- the "current
/// context" toggled by a `!##`/`!^`/`![`/`!]` prefix earlier in the
/// same command (`consume_far_token`), starting at `Active` for every
/// fresh command, matching real Far's own default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelRef {
    Active,
    Passive,
    Left,
    Right,
}


/// Substitutes every macro in `command` against `ctx` -- both real Far
/// Manager's own `!...!` family (confirmed against Far's own
/// `@MetaSymbols` help topic, for reading a ported `FarMenu.ini` or a
/// hand-written command that still uses Far's own syntax) and
/// litastum's own `{{...}}` family (`consume_litastum_token`'s own doc
/// comment explains why this needed a syntax of its own rather than
/// just extending Far's). A `!?...?!` or `{{prompt:...}}` placeholder
/// is deliberately left untouched by this pass -- `super::prompts`'s
/// `extract_prompts`/`substitute_prompts` handle those separately, once
/// the user has actually answered them.
///
/// Deliberate gaps against real Far, all documented at the call site
/// that hits them: short (8.3) filename variants fall back to the long
/// name (no Windows short-name lookup in this codebase, and no such
/// concept at all on macOS/Linux); `!=\`/`!=/` (symbolic-link-resolved
/// path) fall back to the plain path (no symlink-canonicalization
/// helper here yet); `!?!` (Far's separate per-file "description" --
/// `descript.ion` files -- feature) is left as literal text, since
/// litastum has no equivalent; `!@!`/`!$!` ("name of a file containing
/// the list of selected names," Far's own workaround for command-line
/// length limits) fall back to the same inline list `!&` produces,
/// rather than actually writing a scratch file -- keeps this module
/// I/O-free, at the cost of that one narrow use case.
pub fn substitute_macros(command: &str, ctx: &MacroContext) -> String {
    let mut result = String::new();
    let mut rest = command;
    let mut current = PanelRef::Active;

    loop {
        let bang = rest.find('!');
        let brace = rest.find("{{");
        let next = match (bang, brace) {
            (None, None) => break,
            (Some(b), None) => b,
            (None, Some(c)) => c,
            (Some(b), Some(c)) => b.min(c),
        };

        result.push_str(&rest[..next]);
        rest = &rest[next..];

        if rest.starts_with("{{") {
            match consume_litastum_token(rest, ctx, current) {
                Some((text, remainder)) => {
                    result.push_str(&text);
                    rest = remainder;
                }
                None => {
                    // Not a recognized token (or a `{{prompt:...}}`
                    // placeholder, left for later) -- copy the opening
                    // brace pair literally and keep scanning past it.
                    result.push_str("{{");
                    rest = &rest[2..];
                }
            }
            continue;
        }

        let (text, new_current, remainder) = consume_far_token(rest, ctx, current);
        current = new_current;
        result.push_str(&text);
        rest = remainder;
    }

    result.push_str(rest);
    result
}

/// Consumes one Far-style `!...!` (or self-terminating `!&`/`!:`/etc.)
/// macro token starting at `rest` (which must begin with `!`) -- returns
/// the text to emit in its place, the (possibly updated) current-panel
/// context for whatever comes after, and the remainder of `rest` past
/// this token. Order matters: longer/more specific prefixes are checked
/// before shorter ones they'd otherwise be mistaken for (`.!`/`-!`/`+!`
/// before the bare fallback, `&~` before plain `&`, ...), same
/// "backtrack-free, ordered dispatch" shape a real tokenizer needs.
fn consume_far_token<'a>(rest: &'a str, ctx: &MacroContext, current: PanelRef) -> (String, PanelRef, &'a str) {
    debug_assert!(rest.starts_with('!'));
    let after = &rest[1..];

    if let Some(remainder) = after.strip_prefix('!') {
        return ("!".to_string(), current, remainder);
    }
    // `!?...?!` (a real prompt placeholder) and the degenerate `!?!`
    // (Far's file-description macro, unsupported -- see this
    // function's own doc comment) both start with `!?`. Both need to
    // be copied out *atomically*, whole span at once -- copying only
    // the opening `!?` and leaving the rest (including the
    // placeholder's own closing `!`) to be rescanned character-by-
    // character let that closing `!` get misread as a fresh, unrelated
    // macro of its own (e.g. the bare-`!` fallback), silently eating it
    // and leaving `extract_prompts` with no closing `!` to find at all
    // -- reported directly as a real regression: a genuine
    // `!?Label?Default!` placeholder stopped opening the prompt popup
    // and ran the raw, still-un-substituted command instead.
    if after.starts_with('?') {
        if let Some(remainder) = after.strip_prefix("?!") {
            return ("!?!".to_string(), current, remainder);
        }
        if let Some((_, _, _, remainder)) = parse_far_placeholder(rest, 0) {
            let span_len = rest.len() - remainder.len();
            return (rest[..span_len].to_string(), current, remainder);
        }
        // Malformed/incomplete (no closing `!` at all) -- leave just
        // the `!?` as literal text rather than guessing further.
        return ("!?".to_string(), current, &after[1..]);
    }
    // Far's own docs write this prefix as `!##` (two literal `#`
    // characters, confirmed by its own worked examples using it
    // throughout, e.g. "!##!.! denotes the name of the current file on
    // the passive panel") -- not a single `#`.
    if let Some(remainder) = after.strip_prefix("##") {
        return (String::new(), PanelRef::Passive, remainder);
    }
    if let Some(remainder) = after.strip_prefix('^') {
        return (String::new(), PanelRef::Active, remainder);
    }
    if let Some(remainder) = after.strip_prefix('[') {
        return (String::new(), PanelRef::Left, remainder);
    }
    if let Some(remainder) = after.strip_prefix(']') {
        return (String::new(), PanelRef::Right, remainder);
    }

    let panel = ctx.panel(current);

    if let Some(remainder) = after.strip_prefix(".!") {
        return (panel.file_name_with_ext(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix("-!").or_else(|| after.strip_prefix("+!")) {
        return (panel.file_name_with_ext(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix("`~").or_else(|| after.strip_prefix('`')) {
        return (panel.extension(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix('~') {
        return (panel.file_stem(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix("=\\").or_else(|| after.strip_prefix("=/")) {
        return (panel.dir_display(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix('\\').or_else(|| after.strip_prefix('/')) {
        return (panel.dir_display(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix(':') {
        return (panel.drive_display(), current, remainder);
    }
    if let Some(remainder) = after.strip_prefix("&~").or_else(|| after.strip_prefix('&')) {
        let (quote, remainder) = take_list_modifier(remainder);
        return (panel.inline_list(quote), current, remainder);
    }
    if after.starts_with('@') || after.starts_with('$') {
        // "Name of a file containing the list" -- see this function's
        // own doc comment for why this falls back to the plain inline
        // list instead. Far's own docs write the file-list marker
        // doubled ("!@@!") in some places and single ("!$!") in
        // others -- tolerates either one or two marker characters
        // rather than betting on which is the real delimiter, since
        // this macro is already an approximation. Still skips past the
        // modifier letters and the closing `!`, if any, so they don't
        // leak into the substituted command as literal text.
        let remainder = after[1..].trim_start_matches(['@', '$']);
        let after_modifiers = remainder.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        let remainder = after_modifiers.strip_prefix('!').unwrap_or(after_modifiers);
        return (panel.inline_list(true), current, remainder);
    }

    // Bare `!` -- long file name without extension, the fallback once
    // nothing more specific matched (real Far's own reading of an
    // otherwise-unadorned `!`).
    (panel.file_stem(), current, after)
}

/// The single optional modifier letter right after `!&`/`!&~` --
/// `Q` (or no letter at all) quotes each name, `q` leaves them
/// unquoted, matching real Far's own documented default.
fn take_list_modifier(rest: &str) -> (bool, &str) {
    if let Some(remainder) = rest.strip_prefix('q') {
        (false, remainder)
    } else if let Some(remainder) = rest.strip_prefix('Q') {
        (true, remainder)
    } else {
        (true, rest)
    }
}

/// Consumes one litastum-native `{{...}}` macro token starting at
/// `rest` (which must begin with `{{`) -- litastum's own answer to
/// Far's `!...!` macros, picked specifically because `{{`/`}}` collides
/// with nothing cmd.exe (`%VAR%`, or `!VAR!` under delayed expansion),
/// PowerShell (`$var`, `${...}`), or POSIX `sh` (`$var`, `$(...)`,
/// backticks) already give special meaning to -- unlike Far's own
/// `!...!`, which genuinely does collide with cmd's own delayed-
/// expansion `!VAR!` syntax. Currently just the two macros actually
/// requested: `{{cursor}}` (Far's `!.!`) and `{{prompt:...}}` (Far's
/// `!?...?!`, handled by `super::prompts`); more can follow the same
/// pattern later. Returns `None` (leave `{{`/`}}` as two literal
/// characters, and try again one character later) for a
/// `{{prompt:...}}` placeholder (left for `super::prompts`'s
/// `extract_prompts`/`substitute_prompts` to handle once answered) or
/// anything that isn't a recognized token at all.
fn consume_litastum_token<'a>(rest: &'a str, ctx: &MacroContext, current: PanelRef) -> Option<(String, &'a str)> {
    debug_assert!(rest.starts_with("{{"));
    let close = rest.find("}}")?;
    let inside = &rest[2..close];
    let remainder = &rest[close + 2..];

    if inside == "cursor" {
        return Some((ctx.panel(current).file_name_with_ext(), remainder));
    }

    None
}


#[cfg(test)]
mod tests {
    use super::*;

    fn context_with_cursor(path: &str) -> MacroContext {
        let cursor = Some(PathBuf::from(path));
        let panel = PanelMacroContext { dir: PathBuf::from(path).parent().unwrap().to_path_buf(), cursor: cursor.clone(), selected: cursor.into_iter().collect() };
        MacroContext { active: panel.clone(), passive: PanelMacroContext::default(), left: panel.clone(), right: PanelMacroContext::default() }
    }

    #[test]
    fn far_dot_bang_substitutes_the_cursor_files_name_with_extension() {
        let ctx = context_with_cursor("C:/dev/file.txt");
        assert_eq!(substitute_macros("cat !.!", &ctx), "cat file.txt");
    }

    #[test]
    fn far_bare_bang_substitutes_the_cursor_files_name_without_extension() {
        let ctx = context_with_cursor("C:/dev/file.txt");
        assert_eq!(substitute_macros("echo !", &ctx), "echo file");
    }

    /// `!\`` is self-terminating like the bare `!` above -- no
    /// closing `!` of its own (confirmed against Far's own
    /// `@MetaSymbols` help topic).
    #[test]
    fn far_backtick_bang_substitutes_the_extension_only() {
        let ctx = context_with_cursor("C:/dev/file.txt");
        assert_eq!(substitute_macros("echo !`", &ctx), "echo txt");
    }

    /// `!\` (no closing `!`) is Far's own current-path macro --
    /// self-terminating, same as the bare `!`/`!:`/`!&` macros
    /// above.
    #[test]
    fn far_backslash_bang_substitutes_the_current_directory() {
        let ctx = context_with_cursor("C:/dev/file.txt");
        let result = substitute_macros("cd !\\", &ctx);
        assert!(result.contains("dev"), "expected the directory in {result:?}");
    }

    #[test]
    fn far_double_bang_is_a_literal_exclamation_mark() {
        let ctx = MacroContext::default();
        assert_eq!(substitute_macros("echo wow!!", &ctx), "echo wow!");
    }

    #[test]
    fn far_ampersand_substitutes_the_selected_files_quoted_by_default() {
        let mut ctx = MacroContext::default();
        ctx.active.selected = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];
        assert_eq!(substitute_macros("rm !&", &ctx), "rm \"a.txt\" \"b.txt\"");
    }

    #[test]
    fn far_ampersand_lowercase_q_leaves_names_unquoted() {
        let mut ctx = MacroContext::default();
        ctx.active.selected = vec![PathBuf::from("a.txt")];
        assert_eq!(substitute_macros("rm !&q", &ctx), "rm a.txt");
    }

    /// `!##` switches subsequent macros to the passive panel until
    /// the next prefix -- the actual point of the whole panel-
    /// context feature (matches Far's own worked example: "if the
    /// same file exists on the passive panel...").
    #[test]
    fn panel_prefix_toggles_which_panel_later_macros_resolve_against() {
        let mut ctx = MacroContext::default();
        ctx.active.cursor = Some(PathBuf::from("active.txt"));
        ctx.passive.cursor = Some(PathBuf::from("passive.txt"));

        let result = substitute_macros("diff !.! !##!.!", &ctx);

        assert_eq!(result, "diff active.txt passive.txt");
    }

    #[test]
    fn litastum_cursor_macro_substitutes_the_cursor_files_name_with_extension() {
        let ctx = context_with_cursor("C:/dev/file.txt");
        assert_eq!(substitute_macros("cat {{cursor}}", &ctx), "cat file.txt");
    }

    #[test]
    fn litastum_prompt_placeholder_is_left_untouched_for_the_prompt_phase() {
        let ctx = MacroContext::default();
        assert_eq!(substitute_macros("echo {{prompt:Name}}", &ctx), "echo {{prompt:Name}}");
    }

    #[test]
    fn far_prompt_placeholder_is_left_untouched_for_the_prompt_phase() {
        let ctx = MacroContext::default();
        assert_eq!(substitute_macros("echo !?Name?!", &ctx), "echo !?Name?!");
    }

    #[test]
    fn leaves_a_command_with_no_macro_untouched() {
        let ctx = MacroContext::default();
        assert_eq!(substitute_macros("git status -s", &ctx), "git status -s");
    }

    #[test]
    fn an_unrecognized_litastum_style_token_is_left_untouched() {
        let ctx = MacroContext::default();
        assert_eq!(substitute_macros("echo {{nonsense}}", &ctx), "echo {{nonsense}}");
    }
}
