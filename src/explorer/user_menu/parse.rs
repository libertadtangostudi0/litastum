use std::collections::HashSet;
use std::path::PathBuf;

/// One parsed menu entry -- built either by `parse` below (reading a
/// real `FarMenu.ini`, for one-time porting) or by
/// `toml_format::parse_toml` (reading litastum's own
/// `LitastumMenu.toml`). Both file formats resolve to the same
/// in-memory shape, which is what `state.rs`/`input.rs` actually
/// browse and execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// The single character before the item's own `:` (`"s: status"` ->
    /// `Some('s')`), shown as a prefix in the list but not currently
    /// wired up as an instant-select shortcut -- matches this
    /// codebase's own existing precedent (`TODO/f9-menu.md`'s "no
    /// keyboard shortcut letters" gap on the F9 menu too); `Up`/`Down`/
    /// `Enter` is enough for now. `F1`..`F24`-style hotkeys (real Far
    /// Manager also allows those) aren't recognized as hotkeys at all
    /// here -- a line like `F5: refresh` doesn't match the single-char
    /// prefix rule below, so it falls through to being read as a
    /// hotkey-less item titled `F5: refresh`'s remainder, a known,
    /// narrow gap rather than full parity with every hotkey Far allows.
    pub hotkey: Option<char>,
    pub title: String,
    pub body: MenuItemBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemBody {
    /// Every line after the header, in order, until the next header
    /// line, a closing `}`, or end of file -- run in sequence when this
    /// item is selected (`command_line::run_shell_command_lines`).
    Commands(Vec<String>),
    /// A `{ ... }` block right after the header -- its own nested
    /// sequence of items, entered by selecting this one.
    Submenu(Vec<MenuItem>),
}


/// Parses a real `FarMenu.ini`'s content into its top-level items --
/// used only for one-time porting into `LitastumMenu.toml`
/// (`state::port_far_menu`), never for litastum's own file. Far
/// Manager's user-menu format isn't actually `[section]`/`key=value`
/// INI at all (despite the `.ini` extension) -- it's Far's own small
/// nested-block DSL, confirmed against a real published menu
/// (`pkjq/far-git-menu`'s `FarMenu.ini`):
///
/// ```text
/// G: GIT
/// {
/// s: status
/// git status -s
///
/// c: commit
/// {
/// c: Commit
/// git commit -m "!?Commit title?!"
/// }
/// }
/// ```
///
/// - A header line is `<hotkey>: <title>` (`hotkey` is zero or one
///   alphanumeric character right before the *first* `:` on the line —
///   see `is_header_line` for why that's what tells a header apart from
///   an ordinary command line that happens to contain a colon, like a
///   `git log --pretty=format:"..."`).
/// - If the very next line is exactly `{`, the item is a submenu: every
///   line up to the matching `}` is parsed recursively as its own
///   sequence of items.
/// - Otherwise, every following line up to the next header line, a
///   `}`, or end of file is one command to run, in order.
/// - Blank lines separate items but are otherwise ignored; lines whose
///   first non-blank character is `;` are comments, ignored everywhere.
///
/// Malformed input (a line that's neither a valid header nor inside a
/// body -- e.g. stray text before the first header) is skipped rather
/// than causing a parse error or panic; this is a best-effort reader
/// for a hand-edited text file, not a strict format with round-trip
/// guarantees.
pub fn parse(content: &str) -> Vec<MenuItem> {
    let lines: Vec<&str> = content.lines().collect();
    let mut pos = 0;
    parse_items(&lines, &mut pos)
}


fn parse_items(lines: &[&str], pos: &mut usize) -> Vec<MenuItem> {
    let mut items = Vec::new();

    loop {
        skip_blank_and_comment_lines(lines, pos);
        let Some(&line) = lines.get(*pos) else {
            break;
        };
        if line.trim() == "}" {
            break; // caller consumes the closing brace
        }

        let Some((hotkey, title)) = parse_header(line) else {
            // Not a valid header where one was expected -- skip rather
            // than looping forever or misreading it as part of some
            // other item's command body.
            *pos += 1;
            continue;
        };
        *pos += 1;

        if lines.get(*pos).map(|l| l.trim()) == Some("{") {
            *pos += 1;
            let children = parse_items(lines, pos);
            if lines.get(*pos).map(|l| l.trim()) == Some("}") {
                *pos += 1;
            }
            items.push(MenuItem { hotkey, title, body: MenuItemBody::Submenu(children) });
        } else {
            let mut commands = Vec::new();
            while let Some(&line) = lines.get(*pos) {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed == "}" {
                    break;
                }
                if is_comment_line(trimmed) {
                    *pos += 1;
                    continue;
                }
                if is_header_line(line) {
                    break;
                }
                commands.push(line.to_string());
                *pos += 1;
            }
            items.push(MenuItem { hotkey, title, body: MenuItemBody::Commands(commands) });
        }
    }

    items
}


fn skip_blank_and_comment_lines(lines: &[&str], pos: &mut usize) {
    while let Some(&line) = lines.get(*pos) {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_line(trimmed) {
            *pos += 1;
        } else {
            break;
        }
    }
}

fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with(';')
}

/// Whether `line`'s prefix (everything before the first `:`) is a valid
/// hotkey token -- empty, or exactly one alphanumeric character. This
/// is what tells a real header (`"s: status"`, `": log for author"`)
/// apart from a command line that merely contains a colon somewhere
/// (`git log --pretty=format:"..."`, whose prefix up to the first `:`
/// is `"git log --pretty=format"`, nowhere near a valid hotkey).
fn is_header_line(line: &str) -> bool {
    parse_header(line).is_some()
}

fn parse_header(line: &str) -> Option<(Option<char>, String)> {
    let colon = line.find(':')?;
    let prefix = &line[..colon];
    let hotkey = match prefix.chars().count() {
        0 => None,
        1 => {
            let c = prefix.chars().next().unwrap();
            if !c.is_alphanumeric() {
                return None;
            }
            Some(c)
        }
        _ => return None,
    };
    let rest = &line[colon + 1..];
    let title = rest.strip_prefix(' ').unwrap_or(rest).to_string();
    Some((hotkey, title))
}


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
/// is deliberately left untouched by this pass -- `extract_prompts`/
/// `substitute_prompts` handle those separately, once the user has
/// actually answered them.
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
/// `!?...?!`, handled elsewhere -- see below); more can follow the same
/// pattern later. Returns `None` (leave `{{`/`}}` as two literal
/// characters, and try again one character later) for a
/// `{{prompt:...}}` placeholder (left for `extract_prompts`/
/// `substitute_prompts` to handle once answered) or anything that isn't
/// a recognized token at all.
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


/// One `!?Label?Default!` placeholder -- Far Manager's own interactive
/// prompt macro (confirmed against the same real `far-git-menu` file,
/// e.g. `git commit -m "!?Commit title?!"`, `git checkout
/// !?Branch?Master!"`): asks the user for a value (pre-filled with
/// `default`) before running the command, once per unique `label`, and
/// substitutes the answer everywhere that `label` appears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub label: String,
    pub default: String,
}

/// Every unique `!?Label?Default!` placeholder across `commands`, in
/// first-appearance order. The same `label` appearing again (even with
/// a different `default`) isn't asked twice -- its first occurrence's
/// `default` is what's kept, matching Far Manager's own "each distinct
/// label prompts exactly once" behavior.
pub fn extract_prompts(commands: &[String]) -> Vec<Prompt> {
    let mut prompts = Vec::new();
    let mut seen = HashSet::new();

    for command in commands {
        let mut rest = command.as_str();
        while let Some((_prefix, label, default, after)) = next_placeholder(rest) {
            if seen.insert(label.to_string()) {
                prompts.push(Prompt { label: label.to_string(), default: default.to_string() });
            }
            rest = after;
        }
    }

    prompts
}

/// Substitutes every `!?Label?Default!` in `command` with the matching
/// answer from `answers` (an empty string if `command` names a `label`
/// that isn't in `answers` at all -- shouldn't happen in practice,
/// since `answers` is always built from `extract_prompts` run against
/// the very same commands, but a missing answer degrading to empty
/// text is safer than panicking on it).
pub fn substitute_prompts(command: &str, answers: &[(String, String)]) -> String {
    let mut result = String::new();
    let mut rest = command;

    while let Some((prefix, label, _default, after)) = next_placeholder(rest) {
        result.push_str(prefix);
        let answer = answers.iter().find(|(l, _)| l == label).map(|(_, v)| v.as_str()).unwrap_or("");
        result.push_str(answer);
        rest = after;
    }
    result.push_str(rest);

    result
}

/// Finds the next placeholder in `text`, if any -- either Far's own
/// `!?Label?Default!` or litastum's `{{prompt:Label}}`/
/// `{{prompt:Label:Default}}` (`consume_litastum_token`'s own doc
/// comment explains why litastum has its own syntax at all). Returns
/// `(text_before_it, label, default, remainder_after_it)`. Shared by
/// `extract_prompts` (which only reads `label`/`default`) and
/// `substitute_prompts` (which also needs the untouched text on both
/// sides, to rebuild the command with the answer spliced in) -- both
/// work unmodified against whichever syntax was actually used, since
/// this is the only place that has to know both exist.
fn next_placeholder(text: &str) -> Option<(&str, &str, &str, &str)> {
    let far = text.find("!?").and_then(|start| parse_far_placeholder(text, start));
    let native = text.find("{{prompt:").and_then(|start| parse_native_placeholder(text, start));

    match (far, native) {
        (Some(f), Some(n)) => {
            // Whichever one actually starts earlier in `text` wins --
            // `prefix.len()` *is* that starting offset, since both
            // prefixes are slices measured from the very start of
            // `text`.
            if f.0.len() <= n.0.len() { Some(f) } else { Some(n) }
        }
        (Some(f), None) => Some(f),
        (None, Some(n)) => Some(n),
        (None, None) => None,
    }
}

/// Parses a `!?Label?Default!` placeholder assumed to start at byte
/// offset `start` in `text` (`text[start..]` begins with `!?`) -- `None`
/// if what follows isn't actually a well-formed placeholder (e.g. the
/// degenerate `!?!`, Far's own file-description macro, which
/// `substitute_macros` leaves untouched for the same reason).
fn parse_far_placeholder(text: &str, start: usize) -> Option<(&str, &str, &str, &str)> {
    let prefix = &text[..start];
    let after_marker = &text[start + 2..];
    let label_end = after_marker.find('?')?;
    let label = &after_marker[..label_end];
    let after_label = &after_marker[label_end + 1..];
    let default_end = after_label.find('!')?;
    let default = &after_label[..default_end];
    let remainder = &after_label[default_end + 1..];
    Some((prefix, label, default, remainder))
}

/// Parses a `{{prompt:Label}}`/`{{prompt:Label:Default}}` placeholder
/// assumed to start at byte offset `start` in `text`
/// (`text[start..]` begins with `{{prompt:`) -- `None` if there's no
/// matching `}}` at all. A label containing `:` would be ambiguous
/// against its own optional default and isn't attempted here, same
/// narrow scope Far's own `!?Label?Default!` already has (a label
/// containing `?` isn't representable there either).
fn parse_native_placeholder(text: &str, start: usize) -> Option<(&str, &str, &str, &str)> {
    let prefix = &text[..start];
    let after_marker = &text[start + "{{prompt:".len()..];
    let close = after_marker.find("}}")?;
    let inside = &after_marker[..close];
    let remainder = &after_marker[close + 2..];
    let (label, default) = inside.split_once(':').unwrap_or((inside, ""));
    Some((prefix, label, default, remainder))
}


#[cfg(test)]
mod tests {
    use super::*;

    mod parse_tests {
        use super::*;

        #[test]
        fn parses_a_flat_item_with_a_single_command() {
            let items = parse("s: status\ngit status -s\n");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].hotkey, Some('s'));
            assert_eq!(items[0].title, "status");
            assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
        }

        #[test]
        fn parses_an_item_with_no_hotkey() {
            let items = parse(": log for author\ngit log\n");
            assert_eq!(items[0].hotkey, None);
            assert_eq!(items[0].title, "log for author");
        }

        #[test]
        fn parses_multiple_command_lines_for_one_item() {
            let items = parse("u: pull\ngit pull\ngit remote update origin --prune\n");
            assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git pull".to_string(), "git remote update origin --prune".to_string()]));
        }

        #[test]
        fn blank_lines_separate_items_without_affecting_them() {
            let items = parse("s: status\ngit status -s\n\nl: log\ngit log\n");
            assert_eq!(items.len(), 2);
            assert_eq!(items[1].title, "log");
        }

        #[test]
        fn comment_lines_are_ignored_everywhere() {
            let items = parse("; a comment\ns: status\n; another comment\ngit status -s\n");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
        }

        #[test]
        fn parses_a_nested_submenu() {
            let items = parse("c: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n");
            assert_eq!(items.len(), 1);
            let MenuItemBody::Submenu(children) = &items[0].body else { panic!("expected a submenu") };
            assert_eq!(children.len(), 1);
            assert_eq!(children[0].title, "Commit");
        }

        /// Regression coverage for the whole reason `is_header_line`
        /// exists: a real command line containing colons (a
        /// `--pretty=format:"..."` git argument) must not be
        /// misdetected as a new item header and split off from the
        /// item it belongs to.
        #[test]
        fn a_command_line_containing_colons_is_not_mistaken_for_a_header() {
            let items = parse("l: log\ngit log --pretty=format:\"[%h]%ad(%ar) %an : %s\" --graph\n");
            assert_eq!(items.len(), 1, "should still be a single item, not split by the embedded colons");
            assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git log --pretty=format:\"[%h]%ad(%ar) %an : %s\" --graph".to_string()]));
        }

        #[test]
        fn parses_the_real_far_git_menu_example_end_to_end() {
            let content = "G: GIT\n{\ns: status\ngit status -s\n\nc: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n}\n";
            let items = parse(content);
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].title, "GIT");
            let MenuItemBody::Submenu(top_children) = &items[0].body else { panic!("expected a submenu") };
            assert_eq!(top_children.len(), 2);
            assert_eq!(top_children[0].title, "status");
            let MenuItemBody::Submenu(commit_children) = &top_children[1].body else { panic!("expected a nested submenu") };
            assert_eq!(commit_children[0].title, "Commit");
        }

        #[test]
        fn empty_content_parses_to_no_items() {
            assert_eq!(parse(""), Vec::new());
            assert_eq!(parse("\n\n; just a comment\n"), Vec::new());
        }

        #[test]
        fn stray_content_before_any_header_is_skipped_not_fatal() {
            let items = parse("this is not a header\ns: status\ngit status -s\n");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].title, "status");
        }
    }

    mod substitute_macros_tests {
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

    mod prompt_tests {
        use super::*;

        #[test]
        fn extract_prompts_finds_label_and_default() {
            let commands = vec!["git checkout !?Branch?Master!".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }]);
        }

        #[test]
        fn extract_prompts_handles_an_empty_default() {
            let commands = vec!["git commit -m \"!?Commit title?!\"".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "Commit title".to_string(), default: String::new() }]);
        }

        #[test]
        fn extract_prompts_deduplicates_the_same_label_across_commands() {
            let commands = vec!["echo !?Name?!".to_string(), "echo hi !?Name?!".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts.len(), 1, "the same label should only be asked once");
        }

        #[test]
        fn extract_prompts_keeps_first_appearance_order() {
            let commands = vec!["echo !?Second?b! !?First?a!".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), vec!["Second", "First"]);
        }

        #[test]
        fn no_prompts_in_a_plain_command() {
            assert_eq!(extract_prompts(&["git status -s".to_string()]), Vec::new());
        }

        #[test]
        fn substitute_prompts_fills_in_the_answer() {
            let answers = vec![("Branch".to_string(), "feature/x".to_string())];
            assert_eq!(substitute_prompts("git checkout !?Branch?Master!", &answers), "git checkout feature/x");
        }

        #[test]
        fn substitute_prompts_fills_in_every_occurrence_of_the_same_label() {
            let answers = vec![("Name".to_string(), "world".to_string())];
            assert_eq!(substitute_prompts("echo !?Name?! !?Name?!", &answers), "echo world world");
        }

        #[test]
        fn substitute_prompts_uses_empty_string_for_an_unanswered_label() {
            assert_eq!(substitute_prompts("echo !?Name?!", &[]), "echo ");
        }

        #[test]
        fn substitute_prompts_leaves_a_command_with_no_placeholder_untouched() {
            assert_eq!(substitute_prompts("git status -s", &[]), "git status -s");
        }

        /// The actual point of the `{{prompt:...}}` syntax: it goes
        /// through the exact same `extract_prompts`/`substitute_prompts`
        /// path as Far's own `!?...?!`, just spelled differently.
        #[test]
        fn extract_prompts_recognizes_the_litastum_style_syntax_with_a_default() {
            let commands = vec!["git checkout {{prompt:Branch:Master}}".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }]);
        }

        #[test]
        fn extract_prompts_recognizes_the_litastum_style_syntax_with_no_default() {
            let commands = vec!["echo {{prompt:Name}}".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "Name".to_string(), default: String::new() }]);
        }

        #[test]
        fn substitute_prompts_fills_in_the_litastum_style_placeholder() {
            let answers = vec![("Branch".to_string(), "feature/x".to_string())];
            assert_eq!(substitute_prompts("git checkout {{prompt:Branch:Master}}", &answers), "git checkout feature/x");
        }

        /// Both syntaxes can appear in the same command (e.g. a
        /// hand-edited item mixing a freshly-typed `{{prompt:...}}`
        /// with an old, still-present `!?...?!` from a ported
        /// `FarMenu.ini`) -- whichever one starts earlier in the text
        /// is found first, and both eventually get extracted.
        #[test]
        fn extract_prompts_finds_both_syntaxes_in_the_same_command() {
            let commands = vec!["echo !?First?a! {{prompt:Second:b}}".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "First".to_string(), default: "a".to_string() }, Prompt { label: "Second".to_string(), default: "b".to_string() }]);
        }
    }
}
