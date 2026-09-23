use std::fs;
use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::{debug, warn};

use crate::app::{App, Mode};
use crate::history_dir::history_dir;

use super::browsing::{backspace, insert_char, run_command_line};

/// The file name persisted history is stored under, inside
/// `history_dir()`'s own directory.
const HISTORY_FILE: &str = "command_history.txt";

/// Loads persisted history, one command per line, oldest first — same
/// order `record_history` already keeps in memory. A missing/
/// unreachable `history_dir()`, a missing file, or any read error just
/// means "no history yet", not a startup failure (same "log and fall
/// back" shape as `theming::config`'s loaders).
pub fn load_history() -> Vec<String> {
    let Some(dir) = history_dir() else {
        debug!("command history: no history directory available, starting empty");
        return Vec::new();
    };
    match fs::read_to_string(dir.join(HISTORY_FILE)) {
        Ok(contents) => contents.lines().map(str::to_string).collect(),
        Err(err) => {
            debug!(%err, "command history: no persisted history to load");
            Vec::new()
        }
    }
}

/// Best-effort — logged and otherwise ignored on failure, same as
/// `theming::config::try_persist`'s callers; a failed save shouldn't
/// block the command that was just run. Kept separate from
/// `record_history` below (rather than called from inside it) so
/// `record_history`'s own unit tests — which run in parallel and don't
/// want to touch real state — can exercise the in-memory bookkeeping
/// without ever writing this file; the one real caller
/// (`browsing::run_command_line`) calls both in sequence.
pub(super) fn save_history(history: &[String]) {
    let Some(dir) = history_dir() else {
        warn!("command history: no history directory available, not persisted");
        return;
    };
    if let Err(err) = fs::create_dir_all(&dir) {
        warn!(%err, "command history: failed to create the history directory");
        return;
    }
    if let Err(err) = fs::write(dir.join(HISTORY_FILE), history.join("\n")) {
        warn!(%err, "command history: failed to persist");
    }
}

/// Appends `input` to `app.command_history`, for F9 → Commands →
/// History / `Alt+F8` (`handle_history_key`/`draw_command_history`
/// below) — not `Up`-arrow recall, since arrows stay bound to panel
/// navigation on the always-live command line (see the module doc and
/// `.claude/rules/litastum-command-line.md`); a popup has no such
/// conflict, which is what makes History workable at all. Skips a
/// repeat of the immediately-previous entry (typing `dir` three times
/// in a row shouldn't fill History with three identical lines), and
/// caps total length at `theming::config::limits().max_command_history`,
/// dropping the oldest entry once exceeded. Doesn't persist by itself
/// — see `save_history` above.
pub fn record_history(app: &mut App, input: &str) {
    if app.command_history.last().map(String::as_str) == Some(input) {
        return;
    }
    app.command_history.push(input.to_string());
    if app.command_history.len() > crate::theming::config::limits().max_command_history {
        app.command_history.remove(0);
    }
}

/// State for the F9 → Commands → History / `Alt+F8` popup — which row
/// is highlighted. The history itself lives on `App::command_history`
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

/// Entries whose text contains `query` (case-insensitive, substring
/// anywhere — forgiving, same spirit as a shell's own `Ctrl+R` search),
/// in their original oldest-first order. An empty `query` matches
/// everything, which is what makes the popup show the full history
/// before anything's been typed. Shared by `handle_history_key` and
/// `ui::command_line::draw_command_history` so the highlighted index
/// always lines up with what's actually rendered.
pub fn matching_history<'a>(history: &'a [String], query: &str) -> Vec<&'a String> {
    let query = query.to_lowercase();
    history.iter().filter(|entry| entry.to_lowercase().contains(&query)).collect()
}

/// Substring matches (case-insensitive, same as `matching_history`)
/// for the auto-popping suggestion list shown right above the command
/// line while typing (`ui::draw_history_suggestions`) — Far Manager's
/// own command-line autocomplete behaves the same way, appearing
/// unprompted rather than needing an explicit key like `Alt+F8` does.
/// Deduplicated (a command run many times shouldn't clutter the list
/// with repeats) and most-recently-used first, unlike
/// `matching_history`'s oldest-first order — a fresh, unprompted list
/// reads better ordered by recency, same reasoning a shell's own
/// history search prioritizes recent commands. An empty `query`
/// suggests nothing (there's no "you typed nothing" popup — that's
/// what `Alt+F8`'s always-available manual search is for).
pub fn suggest_history<'a>(history: &'a [String], query: &str) -> Vec<&'a str> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut suggestions = Vec::new();
    for entry in history.iter().rev() {
        if entry.to_lowercase().contains(&query_lower) && seen.insert(entry.as_str()) {
            suggestions.push(entry.as_str());
        }
    }
    suggestions
}

/// Indices into `history` of every entry `matching_history` would
/// return, in the same order -- used instead of `matching_history`
/// itself wherever the *original* position (not just the matched text)
/// is needed, e.g. to actually remove the highlighted entry from the
/// real, unfiltered `Vec` (`KeyCode::F(8)` below).
fn matching_history_indices(history: &[String], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    history.iter().enumerate().filter(|(_, entry)| entry.to_lowercase().contains(&query)).map(|(i, _)| i).collect()
}

/// Key handling on the History popup: typing filters the list live
/// (`matching_history`, above) using the same always-live command line
/// everything else types into — Far Manager's own `Alt+F8` behaves the
/// same way, narrowing the list as you type rather than needing a
/// separate search field. `Up`/`Down` move within the *filtered* list,
/// `Enter` runs the highlighted (filtered) entry straight away (through
/// the exact same `run_command_line` the always-live command line's own
/// `Enter` uses, so `cd`/`cls` and the actual shell-out all behave
/// identically) — reported directly as the expected behavior, matching
/// a real shell's own history recall: picking a past command should run
/// it, not just drop it back into the line unexecuted for a second
/// `Enter`. `Tab` instead copies the highlighted entry into the command
/// line for editing *without* running it -- the old `Enter` behavior,
/// moved rather than removed, once `Enter` itself started running
/// things: recalling a command to tweak before running it for real is
/// still a real, separate need from "just run the last one again".
/// `F8` deletes the highlighted entry outright (both in memory and from
/// disk), matching this popup's own F8-deletes convention everywhere
/// else in this app (the file panel's own F8). `Esc` closes without
/// changing the command line.
pub fn handle_history_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    if !matches!(app.mode, Mode::CommandHistory(_)) {
        return Ok(());
    }

    match key.code {
        KeyCode::Up => {
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            crate::list_cursor::move_up(&mut menu.selected);
        }
        KeyCode::Down => {
            let count = matching_history(&app.command_history, &app.command_line).len();
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            crate::list_cursor::move_down(&mut menu.selected, count);
        }
        KeyCode::Enter => {
            let Mode::CommandHistory(menu) = &app.mode else { unreachable!() };
            let selected = menu.selected;
            let entry = matching_history(&app.command_history, &app.command_line).get(selected).map(|entry| (*entry).clone());
            app.command_line_completion = None;
            app.command_line_selection_anchor = None;
            app.mode = Mode::Browsing;
            if let Some(entry) = entry {
                app.command_line = entry;
                app.command_line_cursor = app.command_line.chars().count();
                return run_command_line(app, terminal);
            }
            app.command_line_cursor = app.command_line.chars().count();
        }
        KeyCode::Tab => {
            let Mode::CommandHistory(menu) = &app.mode else { unreachable!() };
            let selected = menu.selected;
            if let Some(entry) = matching_history(&app.command_history, &app.command_line).get(selected) {
                app.command_line = (*entry).clone();
                app.command_line_completion = None;
            }
            app.command_line_cursor = app.command_line.chars().count();
            app.command_line_selection_anchor = None;
            app.mode = Mode::Browsing;
        }
        KeyCode::F(8) => {
            let Mode::CommandHistory(menu) = &app.mode else { unreachable!() };
            let selected = menu.selected;
            let indices = matching_history_indices(&app.command_history, &app.command_line);
            if let Some(&index) = indices.get(selected) {
                app.command_history.remove(index);
                save_history(&app.command_history);
            }
            let new_count = matching_history(&app.command_history, &app.command_line).len();
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = menu.selected.min(new_count.saturating_sub(1));
        }
        KeyCode::Esc => {
            // A selection could have been left active from before Alt+F8
            // opened this popup (`command_line_cursor`/
            // `_selection_anchor` are `Mode::Browsing`-only state, but
            // this popup edits the same `app.command_line` underneath
            // it) -- clear it so `Mode::Browsing` doesn't come back to
            // a stale mid-line cursor or selection the command line here
            // never touched.
            app.command_line_cursor = app.command_line.chars().count();
            app.command_line_selection_anchor = None;
            app.mode = Mode::Browsing;
        }
        KeyCode::Backspace => {
            backspace(&mut app.command_line);
            app.command_line_cursor = app.command_line.chars().count();
            reset_selection(app);
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_char(&mut app.command_line, c);
            app.command_line_cursor = app.command_line.chars().count();
            reset_selection(app);
        }
        _ => {}
    }

    Ok(())
}

/// The filtered list shrinks/reorders as the query changes, so a
/// `selected` index left over from before the last keystroke could
/// point at the wrong row (or past the end of the new list) — jump
/// back to the top of whatever the new filter shows, same as a typical
/// incremental search box.
fn reset_selection(app: &mut App) {
    if let Mode::CommandHistory(menu) = &mut app.mode {
        menu.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_history(history: Vec<&str>) -> App {
        let mut app = test_app(unique_scratch_dir("command-line-history"));
        app.command_history = history.into_iter().map(String::from).collect();
        app
    }

    fn app_in_history_menu(history: Vec<&str>) -> App {
        let mut app = app_with_history(history);
        app.mode = Mode::CommandHistory(CommandHistoryMenu::open());
        app
    }

    mod history_recording_tests {
        use super::*;

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
            let max_history = crate::theming::config::limits().max_command_history;
            let mut app = app_with_history((0..max_history).map(|_| "placeholder").collect());
            // Break up the run of identical "placeholder" entries first, or
            // the immediate-repeat skip above would swallow the new one.
            record_history(&mut app, "distinct");
            assert_eq!(app.command_history.len(), max_history);
            assert_eq!(app.command_history.last().unwrap(), "distinct");
        }
    }

    mod matching_history_tests {
        use super::*;

        #[test]
        fn empty_query_matches_everything_in_original_order() {
            let history = vec!["dir".to_string(), "cargo build".to_string()];
            assert_eq!(matching_history(&history, ""), vec!["dir", "cargo build"]);
        }

        #[test]
        fn query_matches_a_substring_anywhere_case_insensitively() {
            let history = vec!["svn merge -c 1".to_string(), "cargo build".to_string(), "svn status".to_string()];
            assert_eq!(matching_history(&history, "SVN"), vec!["svn merge -c 1", "svn status"]);
        }

        #[test]
        fn no_match_is_an_empty_list() {
            let history = vec!["dir".to_string()];
            assert!(matching_history(&history, "nope").is_empty());
        }
    }

    mod suggest_history_tests {
        use super::*;

        #[test]
        fn matches_a_substring_case_insensitively_most_recent_first() {
            let history = vec!["git status".to_string(), "cargo build".to_string(), "git stash".to_string()];
            assert_eq!(suggest_history(&history, "GIT"), vec!["git stash", "git status"], "most recently used should come first");
        }

        #[test]
        fn deduplicates_repeated_entries() {
            let history = vec!["git status".to_string(), "cargo build".to_string(), "git status".to_string()];
            assert_eq!(suggest_history(&history, "git"), vec!["git status"], "a command run twice shouldn't appear twice");
        }

        #[test]
        fn empty_query_suggests_nothing() {
            let history = vec!["dir".to_string()];
            assert!(suggest_history(&history, "").is_empty());
        }

        #[test]
        fn no_match_suggests_nothing() {
            let history = vec!["dir".to_string()];
            assert!(suggest_history(&history, "nope").is_empty());
        }

        #[test]
        fn matches_a_substring_anywhere_not_just_a_prefix() {
            let history = vec!["cargo build".to_string()];
            assert_eq!(suggest_history(&history, "build"), vec!["cargo build"]);
        }
    }

    mod history_key_handling_tests {
        use super::*;

        /// A throwaway `Terminal` for handlers that need one just to
        /// satisfy the signature -- never actually drawn to. Every test
        /// here recalls a `cd`-shaped entry specifically so
        /// `run_command_line` takes its early `Panel::change_dir`
        /// return, never reaching the real-subprocess `run_shell_command_lines`
        /// path (which needs a real console -- same limitation
        /// `command_line::browsing`'s own tests already accept, and
        /// `explorer::user_menu::input`'s own `dummy_terminal` doc
        /// comment explains the same way).
        fn dummy_terminal() -> Terminal<CrosstermBackend<Stdout>> {
            Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap()
        }

        /// Real reported behavior, by analogy with a shell's own history
        /// recall: selecting a past command should run it immediately,
        /// not just drop it back into the line unexecuted.
        #[test]
        fn handle_history_key_enter_runs_the_selected_entry() {
            let mut app = app_in_history_menu(vec!["dir", "cd nowhere"]);
            let target = app.panels[app.active].path.join("sub");
            fs::create_dir_all(&target).unwrap();
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = 1;
            // Overwrite the second entry with a target that actually
            // exists, so the effect of running it is observable.
            app.command_history[1] = "cd sub".to_string();

            handle_history_key(&mut app, key(KeyCode::Enter), &mut dummy_terminal()).unwrap();

            assert_eq!(app.panels[app.active].path, target, "Enter should have actually run the recalled cd, not just copied it");
            assert_eq!(app.command_line, "", "run_command_line clears the line once it's actually run");
            assert!(matches!(app.mode, Mode::Browsing));
        }

        #[test]
        fn handle_history_key_esc_cancels_without_changing_the_command_line() {
            let mut app = app_in_history_menu(vec!["dir"]);
            app.command_line = "untouched".to_string();

            handle_history_key(&mut app, key(KeyCode::Esc), &mut dummy_terminal()).unwrap();

            assert_eq!(app.command_line, "untouched");
            assert!(matches!(app.mode, Mode::Browsing));
        }

        #[test]
        fn handle_history_key_down_is_clamped_at_the_last_entry() {
            let mut app = app_in_history_menu(vec!["a", "b"]);
            for _ in 0..5 {
                handle_history_key(&mut app, key(KeyCode::Down), &mut dummy_terminal()).unwrap();
            }
            let Mode::CommandHistory(menu) = &app.mode else { panic!("expected Mode::CommandHistory") };
            assert_eq!(menu.selected, 1);
        }

        #[test]
        fn handle_history_key_is_a_noop_outside_command_history_mode() {
            let mut app = app_in_history_menu(vec!["dir"]);
            app.mode = Mode::Browsing;

            handle_history_key(&mut app, key(KeyCode::Enter), &mut dummy_terminal()).unwrap();

            assert!(matches!(app.mode, Mode::Browsing));
            assert_eq!(app.command_line, "");
        }

        /// The actual reported behavior: typing narrows the popup's list
        /// live, using the same command line everything else types into.
        #[test]
        fn typing_filters_the_list_and_resets_the_selection() {
            let mut app = app_in_history_menu(vec!["svn merge -c 1", "cargo build", "svn status"]);
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = 1; // "cargo build", before any filtering

            handle_history_key(&mut app, key(KeyCode::Char('s')), &mut dummy_terminal()).unwrap();
            handle_history_key(&mut app, key(KeyCode::Char('v')), &mut dummy_terminal()).unwrap();
            handle_history_key(&mut app, key(KeyCode::Char('n')), &mut dummy_terminal()).unwrap();

            assert_eq!(app.command_line, "svn");
            let Mode::CommandHistory(menu) = &app.mode else { panic!("expected Mode::CommandHistory") };
            assert_eq!(menu.selected, 0, "selection should reset once the filter narrows the list");
        }

        /// Real reported behavior, by analogy with Far Manager: `Enter`
        /// runs whichever entry is highlighted in the *filtered* list,
        /// not whatever was highlighted before the filter narrowed it.
        #[test]
        fn enter_on_a_filtered_match_runs_that_match_not_the_pre_filter_selection() {
            let mut app = app_in_history_menu(vec!["cd sub1", "cargo build", "cd sub2"]);
            let base = app.panels[app.active].path.clone();
            fs::create_dir_all(base.join("sub1")).unwrap();
            fs::create_dir_all(base.join("sub2")).unwrap();

            for c in "cd s".chars() {
                handle_history_key(&mut app, key(KeyCode::Char(c)), &mut dummy_terminal()).unwrap();
            }
            // Filtered list is now ["cd sub1", "cd sub2"]; arrow down to
            // the second match before running it.
            handle_history_key(&mut app, key(KeyCode::Down), &mut dummy_terminal()).unwrap();

            handle_history_key(&mut app, key(KeyCode::Enter), &mut dummy_terminal()).unwrap();

            assert_eq!(app.panels[app.active].path, base.join("sub2"), "Enter should have run the highlighted filtered match (\"cd sub2\"), not the first entry");
            assert!(matches!(app.mode, Mode::Browsing));
        }

        /// `Tab` is the old `Enter` behavior, moved rather than removed
        /// once `Enter` itself started running things directly -- still
        /// needed for recalling a command to tweak before running it.
        #[test]
        fn handle_history_key_tab_copies_the_selected_entry_without_running_it() {
            let mut app = app_in_history_menu(vec!["dir", "cd nonexistent-dir"]);
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = 1;
            let original_path = app.panels[app.active].path.clone();

            handle_history_key(&mut app, key(KeyCode::Tab), &mut dummy_terminal()).unwrap();

            assert_eq!(app.command_line, "cd nonexistent-dir");
            assert_eq!(app.panels[app.active].path, original_path, "Tab must not run the command");
            assert!(matches!(app.mode, Mode::Browsing));
        }

        /// Real requested behavior: `F8` deletes the highlighted entry,
        /// matching this app's own F8-deletes convention (the file
        /// panel's own F8).
        #[test]
        fn handle_history_key_f8_deletes_the_selected_entry() {
            let mut app = app_in_history_menu(vec!["dir", "cargo build", "git status"]);
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = 1;

            handle_history_key(&mut app, key(KeyCode::F(8)), &mut dummy_terminal()).unwrap();

            assert_eq!(app.command_history, vec!["dir", "git status"]);
            assert!(matches!(app.mode, Mode::CommandHistory(_)), "F8 should delete in place, not close the popup");
        }

        /// `F8` operates on the *filtered* list's own indices, not the
        /// unfiltered history's -- deleting the wrong entry (or panicking
        /// on an out-of-bounds index) would be the failure mode if this
        /// resolved `selected` against `app.command_history` directly.
        #[test]
        fn handle_history_key_f8_deletes_the_filtered_match_not_the_wrong_entry() {
            let mut app = app_in_history_menu(vec!["cd sub1", "cargo build", "cd sub2"]);

            for c in "cd s".chars() {
                handle_history_key(&mut app, key(KeyCode::Char(c)), &mut dummy_terminal()).unwrap();
            }
            // Filtered list is now ["cd sub1", "cd sub2"]; arrow down to
            // the second match before deleting it.
            handle_history_key(&mut app, key(KeyCode::Down), &mut dummy_terminal()).unwrap();

            handle_history_key(&mut app, key(KeyCode::F(8)), &mut dummy_terminal()).unwrap();

            assert_eq!(app.command_history, vec!["cd sub1", "cargo build"], "should have deleted \"cd sub2\", not \"cargo build\"");
        }

        /// Deleting the last remaining filtered match must re-clamp
        /// `selected` instead of leaving it pointing past the now-shorter
        /// filtered list.
        #[test]
        fn handle_history_key_f8_reclamps_selection_after_deleting_the_last_match() {
            let mut app = app_in_history_menu(vec!["dir", "cargo build"]);
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = 1;

            handle_history_key(&mut app, key(KeyCode::F(8)), &mut dummy_terminal()).unwrap();

            assert_eq!(app.command_history, vec!["dir"]);
            let Mode::CommandHistory(menu) = &app.mode else { panic!("expected Mode::CommandHistory") };
            assert_eq!(menu.selected, 0);
        }
    }
}
