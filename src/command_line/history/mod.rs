use std::fs;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, warn};

use crate::app::{App, Mode};
use crate::history_dir::history_dir;

use super::browsing::submit_command_line;
use super::effect::Effect;

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
/// (`browsing::submit_command_line`) calls both in sequence.
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
/// cursor position.
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
/// the exact same `submit_command_line` the always-live command line's own
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
pub fn handle_history_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if !matches!(app.mode, Mode::CommandHistory(_)) {
        return Ok(Effect::None);
    }

    match key.code {
        KeyCode::Up => {
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            crate::list_cursor::move_up(&mut menu.selected);
        }
        KeyCode::Down => {
            let count = matching_history(&app.command_history, app.command_line.text()).len();
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            crate::list_cursor::move_down(&mut menu.selected, count);
        }
        KeyCode::Enter => {
            let Mode::CommandHistory(menu) = &app.mode else { unreachable!() };
            let selected = menu.selected;
            let entry = matching_history(&app.command_history, app.command_line.text()).get(selected).map(|entry| (*entry).clone());
            app.command_line_completion = None;
            app.mode = Mode::Browsing;
            if let Some(entry) = entry {
                app.command_line.set_text(entry);
                return submit_command_line(app);
            }
            app.command_line.move_to_end();
        }
        KeyCode::Tab => {
            let Mode::CommandHistory(menu) = &app.mode else { unreachable!() };
            let selected = menu.selected;
            if let Some(entry) = matching_history(&app.command_history, app.command_line.text()).get(selected) {
                app.command_line.set_text((*entry).clone());
                app.command_line_completion = None;
            }
            app.command_line.move_to_end();
            app.mode = Mode::Browsing;
        }
        KeyCode::F(8) => {
            let Mode::CommandHistory(menu) = &app.mode else { unreachable!() };
            let selected = menu.selected;
            let indices = matching_history_indices(&app.command_history, app.command_line.text());
            if let Some(&index) = indices.get(selected) {
                app.command_history.remove(index);
                save_history(&app.command_history);
            }
            let new_count = matching_history(&app.command_history, app.command_line.text()).len();
            let Mode::CommandHistory(menu) = &mut app.mode else { unreachable!() };
            menu.selected = menu.selected.min(new_count.saturating_sub(1));
        }
        KeyCode::Esc => {
            // This popup edits `app.command_line` as a plain filter, so
            // don't return to a stale mid-line cursor or selection.
            app.command_line.move_to_end();
            app.mode = Mode::Browsing;
        }
        KeyCode::Backspace => {
            app.command_line.pop_char();
            reset_selection(app);
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.command_line.push_char(c);
            reset_selection(app);
        }
        _ => {}
    }

    Ok(Effect::None)
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
mod tests;
