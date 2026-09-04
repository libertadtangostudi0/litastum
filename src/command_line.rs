//! The always-live command line at the bottom of the browser (Far
//! Manager-style — see `.claude/rules/litastum-stack.md` for why it's
//! append/backspace-only, no cursor movement).
//!
//! `insert_char`/`backspace`/`parse_cd_target` below are pure and
//! tested without a terminal; `handle_browsing_key`/`run_command_line`
//! are the impure key-handling/process-spawning half, moved here from
//! `main.rs` so this module owns everything about the command line
//! (mirrors `theme_menu.rs`/`menu.rs`, which each own their state *and*
//! key handling — `main.rs` stays a thin dispatcher).

use std::fs;
use std::io::Stdout;
use std::path::Path;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tracing::debug;

use crate::app::{App, Mode, ShellMenu};
use crate::theme::Theme;
use crate::{command, keymap};


/// Key handling in the browser: `Ctrl+P` opens the shell picker,
/// `Shift+F6` opens the rename prompt (both need the raw modifier,
/// which `keymap::resolve`'s table can't see since it only keys off
/// `KeyCode`), `Enter` with something typed runs it (`run_command_line`),
/// otherwise the fixed `keymap::resolve` table (arrows, Tab, F4/F9/F10,
/// and `Enter` on an *empty* command line — `EnterSelected`, unchanged)
/// takes over; anything that table doesn't bind — plain characters,
/// `Backspace`, `Esc` — edits the always-live command line at the
/// bottom of the browser. This is also why `q` no longer quits on its
/// own (`keymap.rs`) — a bare letter now types into the command line
/// like any other.
pub fn handle_browsing_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    debug!(?key, "browsing key");

    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.mode = Mode::ShellMenu(ShellMenu { selected: app.active_shell });
        return Ok(());
    }

    // Shift+F6 (rename) vs plain F6 (move) only differ by modifier --
    // keymap::resolve's table keys off KeyCode alone, so this one has
    // to be special-cased ahead of it, same as Ctrl+P above.
    if key.code == KeyCode::F(6) && key.modifiers.contains(KeyModifiers::SHIFT) {
        return command::execute(keymap::Command::RenameSelected, app);
    }

    if key.code == KeyCode::Enter && !app.command_line.is_empty() {
        app.command_line_completion = None;
        return run_command_line(app, terminal);
    }

    // Tab completes the command line's typed text (below) while
    // there's something to complete; only falls through to
    // `keymap::resolve`'s Tab-as-ToggleActive binding once the line is
    // empty. Without this check, Tab always switched panels, even
    // mid-command -- exactly backwards from every shell's own
    // convention for the key.
    if key.code == KeyCode::Tab && !app.command_line.is_empty() {
        let cwd = app.active_panel().path.clone();
        complete(&mut app.command_line, &cwd, &mut app.command_line_completion);
        return Ok(());
    }

    if let Some(cmd) = keymap::resolve(key.code) {
        debug!(?cmd, "browsing command");
        // Any bound command (including plain Tab on an empty line,
        // switching panels) leaves the command line untouched, so a
        // stale completion cycle wouldn't otherwise get cleared here --
        // but there's nothing left to cycle through once the mode
        // changes or the panel does, so drop it regardless.
        app.command_line_completion = None;
        return command::execute(cmd, app);
    }

    match key.code {
        KeyCode::Esc => {
            app.command_line.clear();
            app.command_line_completion = None;
        }
        KeyCode::Backspace => {
            backspace(&mut app.command_line);
            app.command_line_completion = None;
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_char(&mut app.command_line, c);
            app.command_line_completion = None;
        }
        _ => {}
    }

    Ok(())
}


/// Runs whatever's typed in `app.command_line`: `cd`-shaped input
/// changes the active panel's directory directly (`Panel::change_dir`
/// — a spawned shell's own `cd` could never affect our process, so
/// this has to be handled ourselves, same as Far Manager does it);
/// `cls`/`clear` repaint the TUI directly (below) rather than actually
/// shelling out; anything else suspends the TUI and hands the console
/// to the configured shell profile (`app.shell_profiles[app.active_shell]`),
/// inheriting stdio so interactive programs (an editor, a REPL, ...)
/// work too, not just one-shot commands.
fn run_command_line(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let input = app.command_line.trim().to_string();
    app.command_line.clear();
    if input.is_empty() {
        return Ok(());
    }
    record_history(app, &input);

    if let Some(target) = parse_cd_target(&input) {
        debug!(target, "command line: cd");
        app.active_panel().change_dir(target)?;
        return Ok(());
    }

    // A screen-clear command's entire job is leaving nothing on
    // screen -- shelling out to a real `cls`/`clear` did exactly that,
    // including wiping the "{cwd}> cls" prompt line printed just below
    // for every other command, and then the "Press any key to
    // continue..." pause (which exists so real command *output* isn't
    // lost the instant the panels redraw over it) had nothing left to
    // protect -- just a stray message floating on an otherwise blank
    // screen. Reported as a confusing/broken-looking screen; fixed by
    // never leaving the TUI for these two at all, matching what the
    // command is actually trying to accomplish (a repaint) far more
    // directly than round-tripping through a real subprocess.
    if input == "cls" || input == "clear" {
        debug!("command line: clear screen (handled directly, no subprocess)");
        terminal.clear()?;
        app.active_panel().reload()?;
        return Ok(());
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    debug!(shell = profile.name, %input, cwd = %cwd.display(), "command line: running");

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    println!("{}> {input}", cwd.display());
    let status = std::process::Command::new(&profile.program)
        .args(&profile.args_prefix)
        .arg(&input)
        .current_dir(&cwd)
        .status();
    match status {
        Ok(status) if !status.success() => {
            debug!(?status, "command exited non-zero");
        }
        Err(err) => println!("failed to launch '{}': {err}", profile.program),
        Ok(_) => {}
    }
    println!("\nPress any key to continue...");

    // Wait for one real keypress before redrawing -- otherwise output
    // that scrolled by fast is gone the instant the panels repaint.
    loop {
        if let Event::Key(k) = event::read()? {
            if k.kind == KeyEventKind::Press {
                break;
            }
        }
    }

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    app.active_panel().reload()?;
    Ok(())
}


/// Longest `App::command_history` is allowed to grow — oldest entries
/// drop off the front once exceeded.
const MAX_HISTORY: usize = 50;


/// Appends `input` to `app.command_history`, for F9 → Commands →
/// History (`handle_history_key`/`draw_command_history` below) — not
/// `Up`-arrow recall, since arrows stay bound to panel navigation on
/// the always-live command line (see the module doc and
/// `.claude/rules/litastum-command-line.md`); a popup has no such
/// conflict, which is what makes History workable at all. Skips a
/// repeat of the immediately-previous entry (typing `dir` three times
/// in a row shouldn't fill History with three identical lines), and
/// caps total length at `MAX_HISTORY`, dropping the oldest entry once
/// exceeded.
fn record_history(app: &mut App, input: &str) {
    if app.command_history.last().map(String::as_str) == Some(input) {
        return;
    }
    app.command_history.push(input.to_string());
    if app.command_history.len() > MAX_HISTORY {
        app.command_history.remove(0);
    }
}


/// State for the F9 → Commands → History popup — which row is
/// highlighted. The history itself lives on `App::command_history`
/// (recorded by `record_history` above), not here — this is just a
/// cursor position, same shape as `app::ShellMenu`.
pub struct CommandHistoryMenu {
    pub selected: usize,
}


impl CommandHistoryMenu {
    pub fn open() -> Self {
        Self { selected: 0 }
    }
}


/// Key handling on the History popup: `Up`/`Down` move, `Enter` copies
/// the highlighted entry into `app.command_line` for editing/running
/// (doesn't run it immediately — recalling a command to tweak it before
/// pressing `Enter` for real is the more common case, and never running
/// something automatically is the safer default regardless), `Esc`
/// closes without changing the command line.
pub fn handle_history_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::CommandHistory(menu) = &mut app.mode else {
        return Ok(());
    };

    match key.code {
        KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
        KeyCode::Down => {
            if menu.selected + 1 < app.command_history.len() {
                menu.selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = app.command_history.get(menu.selected).cloned() {
                app.command_line = entry;
                app.command_line_completion = None;
            }
            app.mode = Mode::Browsing;
        }
        KeyCode::Esc => app.mode = Mode::Browsing,
        _ => {}
    }

    Ok(())
}


/// Renders the History popup — most-recently-run command last (natural
/// reading order for "what did I just type"), or a hint that nothing's
/// been run yet.
pub fn draw_command_history(frame: &mut Frame, area: Rect, menu: &CommandHistoryMenu, history: &[String], theme: &Theme) {
    let height = (history.len().max(1) as u16 + 4).clamp(6, area.height);
    let popup = crate::ui::centered_rect(60, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" History ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if history.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled("No commands run yet", Style::default().fg(theme.text_dim))));
        frame.render_widget(empty, rows[0]);
    } else {
        let items: Vec<ListItem> = history
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let style = if index == menu.selected {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(entry.clone(), style)))
            })
            .collect();
        frame.render_widget(List::new(items), rows[0]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" recall  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


/// Appends `c` to the typed command.
pub fn insert_char(line: &mut String, c: char) {
    line.push(c);
}


/// Removes the last character, if any. A no-op on an empty line.
pub fn backspace(line: &mut String) {
    line.pop();
}


/// A live `Tab`-cycling session, remembered on `App` across repeated
/// `Tab` presses (`App::command_line_completion`) — Windows `cmd.exe`
/// convention: the first `Tab` on a word shows the first match, each
/// further `Tab` steps to the next one (wrapping back to the first
/// after the last), rather than completing to the matches' shared
/// prefix and stopping. Any other edit to the command line
/// (`insert_char`/`backspace`/`Esc`/running it) ends the session — see
/// `handle_browsing_key`.
pub struct CompletionCycle {
    /// Where the word being completed starts in the line — constant
    /// for the life of one cycle, since every step replaces
    /// `line[word_start..]` wholesale rather than editing in place.
    word_start: usize,
    /// The directory portion of the word as typed (e.g. `"sub\"` in
    /// `"cd sub\tar"`), re-prepended in front of each match in turn.
    dir_part: String,
    /// Every entry in `dir_part` whose name matched the typed prefix
    /// when the cycle started, name plus whether it's a directory
    /// (decides the trailing separator vs. space — see `apply`).
    matches: Vec<(String, bool)>,
    index: usize,
}


/// `Tab`: continues `cycle` if one's already running (steps to the
/// next match), otherwise starts a new one from the word currently
/// under the cursor — the last whitespace-separated "word" in `line`,
/// resolved as a filesystem path relative to `cwd` (an already-absolute
/// word, like `C:\Users\` or `/etc/`, completes from its own root
/// instead — `Path::join`'s own behavior, same as `Panel::change_dir`'s
/// `cd` handling relies on). No matches leaves `line` and `cycle`
/// untouched entirely — no bell, no error, nothing suggested.
///
/// Matching is case-insensitive (Windows filesystems don't
/// distinguish; harmless extra leniency on case-sensitive ones too).
pub fn complete(line: &mut String, cwd: &Path, cycle: &mut Option<CompletionCycle>) {
    if let Some(state) = cycle {
        state.index = (state.index + 1) % state.matches.len();
        apply(line, state);
        return;
    }

    let word_start = line.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let word = &line[word_start..];
    if word.is_empty() {
        return;
    }

    let dir_end = word.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (dir_part, prefix) = word.split_at(dir_end);
    let search_dir = cwd.join(dir_part);

    let Ok(entries) = fs::read_dir(&search_dir) else {
        return;
    };

    let mut matches: Vec<(String, bool)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.to_lowercase()
                .starts_with(&prefix.to_lowercase())
                .then(|| (name, entry.file_type().is_ok_and(|t| t.is_dir())))
        })
        .collect();
    if matches.is_empty() {
        return;
    }
    matches.sort();

    let state = CompletionCycle { word_start, dir_part: dir_part.to_string(), matches, index: 0 };
    apply(line, &state);
    *cycle = Some(state);
}


/// Replaces `line`'s current word with `state`'s currently-selected
/// match — a trailing path separator for a directory (so the next
/// `Tab` press, or typed character, continues *inside* it) or a
/// trailing space for a file (ready for the next argument), matching a
/// normal shell's own completion habit.
fn apply(line: &mut String, state: &CompletionCycle) {
    let (name, is_dir) = &state.matches[state.index];
    let trailer = if *is_dir { std::path::MAIN_SEPARATOR } else { ' ' };
    line.truncate(state.word_start);
    line.push_str(&state.dir_part);
    line.push_str(name);
    line.push(trailer);
}


/// If `input` is a `cd` command, returns its argument (trimmed) — or
/// `None` for the argument-less `"cd"` (a no-op, not "go home"; see
/// the plan doc), and `None` for anything that isn't `cd` at all
/// (including a different command that merely starts with "cd", like
/// `"cdw"` — checked via a word boundary, not a bare prefix).
pub fn parse_cd_target(input: &str) -> Option<&str> {
    let rest = input.strip_prefix("cd")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None; // e.g. "cdw ..", not "cd"
    }
    let target = rest.trim();
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A fresh scratch directory under the OS temp dir, unique per test
    /// (same pattern as `panel.rs`/`fs_ops.rs`'s own scratch helpers).
    fn scratch_dir() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litastum-command-line-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn complete_single_directory_match_appends_a_trailing_separator() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("source")).unwrap();
        let mut line = "cd sou".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cd source{}", std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn complete_single_file_match_appends_a_trailing_space() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();
        let mut line = "cat read".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, "cat readme.txt ");
    }

    #[test]
    fn complete_no_matches_leaves_the_line_untouched() {
        let dir = scratch_dir();
        let mut line = "cat nope".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, "cat nope");
        assert!(cycle.is_none(), "nothing to cycle through");
    }

    #[test]
    fn complete_only_touches_the_last_word() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("source")).unwrap();
        let mut line = "cp already-typed sou".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cp already-typed source{}", std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn complete_matches_case_insensitively() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("Source")).unwrap();
        let mut line = "cd sou".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cd Source{}", std::path::MAIN_SEPARATOR), "should find it despite the case mismatch");
    }

    #[test]
    fn complete_descends_into_an_explicitly_typed_subdirectory() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("sub").join("target")).unwrap();
        let separator = std::path::MAIN_SEPARATOR;
        let mut line = format!("cd sub{separator}tar");
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cd sub{separator}target{separator}"));
    }

    #[test]
    fn complete_on_an_empty_word_is_a_noop() {
        let dir = scratch_dir();
        let mut line = "cd ".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, "cd ");
    }

    /// The actual point of this session: with several matches, repeated
    /// `Tab` presses (repeated `complete` calls sharing the same
    /// `cycle`) step through *every* match in turn — not just complete
    /// to their shared prefix once and stop, cmd.exe's own convention
    /// for the key.
    #[test]
    fn complete_cycles_through_every_match_on_repeated_tab() {
        let dir = scratch_dir();
        fs::write(dir.join("Cargo.lock"), b"").unwrap();
        fs::write(dir.join("Cargo.toml"), b"").unwrap();
        let mut line = ".\\Cargo.".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, ".\\Cargo.lock ", "first Tab: first match alphabetically");

        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, ".\\Cargo.toml ", "second Tab: the other match");

        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, ".\\Cargo.lock ", "third Tab: wraps back around to the first");
    }

    /// A completion cycle survives further `Tab` presses but nothing
    /// else — `handle_browsing_key` is what actually enforces "any
    /// other edit ends it" (it owns `App::command_line_completion`),
    /// this only pins down that `complete` itself doesn't reset
    /// `cycle` on repeated calls unless told to via a fresh `None`.
    #[test]
    fn a_fresh_none_cycle_starts_over_instead_of_continuing() {
        let dir = scratch_dir();
        fs::write(dir.join("Cargo.lock"), b"").unwrap();
        fs::write(dir.join("Cargo.toml"), b"").unwrap();
        let mut line = "Cargo.".to_string();
        let mut cycle = None;
        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, "Cargo.lock ");

        line = "Cargo.".to_string();
        let mut fresh_cycle = None;
        complete(&mut line, &dir, &mut fresh_cycle);

        assert_eq!(line, "Cargo.lock ", "starts back at the first match, not continuing the old cycle");
    }

    #[test]
    fn insert_char_appends() {
        let mut line = String::from("di");
        insert_char(&mut line, 'r');
        assert_eq!(line, "dir");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut line = String::from("dir");
        backspace(&mut line);
        assert_eq!(line, "di");
    }

    #[test]
    fn backspace_on_empty_line_is_a_noop() {
        let mut line = String::new();
        backspace(&mut line);
        assert_eq!(line, "");
    }

    #[test]
    fn parse_cd_target_extracts_the_argument() {
        assert_eq!(parse_cd_target("cd .."), Some(".."));
        assert_eq!(parse_cd_target("cd src"), Some("src"));
        assert_eq!(parse_cd_target("cd   spaced   "), Some("spaced"));
    }

    #[test]
    fn parse_cd_target_bare_cd_is_none() {
        assert_eq!(parse_cd_target("cd"), None);
        assert_eq!(parse_cd_target("cd   "), None);
    }

    #[test]
    fn parse_cd_target_rejects_other_commands() {
        assert_eq!(parse_cd_target("cdw --version"), None);
        assert_eq!(parse_cd_target("cargo build"), None);
        assert_eq!(parse_cd_target(""), None);
    }

    #[test]
    fn parse_cd_target_is_case_sensitive() {
        // Matches cmd.exe/sh convention (cd is lowercase); CD/Cd are
        // handled fine by cmd.exe itself if shelled out instead.
        assert_eq!(parse_cd_target("CD src"), None);
    }

    fn app_with_history(history: Vec<&str>) -> App {
        let dir = scratch_dir();
        let mut app = App::new(dir, Theme::dark(), None).expect("build app");
        app.command_history = history.into_iter().map(String::from).collect();
        app
    }

    #[test]
    fn record_history_appends_new_commands() {
        let mut app = app_with_history(vec!["dir"]);
        record_history(&mut app, "cargo build");
        assert_eq!(app.command_history, vec!["dir", "cargo build"]);
    }

    #[test]
    fn record_history_skips_an_immediate_repeat() {
        let mut app = app_with_history(vec!["dir"]);
        record_history(&mut app, "dir");
        assert_eq!(app.command_history, vec!["dir"], "typing the same command twice shouldn't duplicate it");
    }

    #[test]
    fn record_history_allows_a_repeat_that_is_not_immediately_consecutive() {
        let mut app = app_with_history(vec!["dir", "cargo build"]);
        record_history(&mut app, "dir");
        assert_eq!(app.command_history, vec!["dir", "cargo build", "dir"]);
    }

    #[test]
    fn record_history_caps_at_max_history_dropping_the_oldest() {
        let mut app = app_with_history((0..MAX_HISTORY).map(|_| "placeholder").collect());
        // Break up the run of identical "placeholder" entries first, or
        // the immediate-repeat skip above would swallow the new one.
        record_history(&mut app, "distinct");
        assert_eq!(app.command_history.len(), MAX_HISTORY);
        assert_eq!(app.command_history.last().unwrap(), "distinct");
    }

    fn app_in_history_menu(history: Vec<&str>) -> App {
        let mut app = app_with_history(history);
        app.mode = Mode::CommandHistory(CommandHistoryMenu::open());
        app
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn handle_history_key_enter_copies_the_selected_entry_into_the_command_line() {
        let mut app = app_in_history_menu(vec!["dir", "cargo build"]);
        let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
        menu.selected = 1;

        handle_history_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.command_line, "cargo build");
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_history_key_enter_does_not_run_the_command() {
        // Recall should let the user review/edit before running it --
        // never auto-execute. Confirmed indirectly: Mode::Browsing (not
        // some "running" state) and the panel's untouched cwd is the
        // only way to observe this without a real Terminal.
        let mut app = app_in_history_menu(vec!["cd nonexistent-dir"]);
        let original_path = app.panels[app.active].path.clone();

        handle_history_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.panels[app.active].path, original_path);
    }

    #[test]
    fn handle_history_key_esc_cancels_without_changing_the_command_line() {
        let mut app = app_in_history_menu(vec!["dir"]);
        app.command_line = "untouched".to_string();

        handle_history_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert_eq!(app.command_line, "untouched");
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_history_key_down_is_clamped_at_the_last_entry() {
        let mut app = app_in_history_menu(vec!["a", "b"]);
        for _ in 0..5 {
            handle_history_key(&mut app, key(KeyCode::Down)).unwrap();
        }
        let Mode::CommandHistory(menu) = &app.mode else { panic!("expected Mode::CommandHistory") };
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn handle_history_key_is_a_noop_outside_command_history_mode() {
        let mut app = app_in_history_menu(vec!["dir"]);
        app.mode = Mode::Browsing;

        handle_history_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert_eq!(app.command_line, "");
    }
}
