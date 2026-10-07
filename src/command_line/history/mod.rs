use std::fs;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{debug, warn};

use crate::app::{App, Overlay};
use crate::app_data::history_dir;

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

/// Best-effort: a failed save is logged. Separate from `record_history` so
/// its tests stay off the disk; `submit_command_line` calls both.
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

/// Appends `input` to the history for F9 -> Commands -> History /
/// `Alt+F8` (not `Up` recall: arrows navigate the panels). Skips a repeat
/// of the last entry, caps at `limits().max_command_history`. Doesn't save
/// (`save_history`).
pub fn record_history(app: &mut App, input: &str) {
    if app.command_history.last().map(String::as_str) == Some(input) {
        return;
    }
    app.command_history.push(input.to_string());
    if app.command_history.len() > crate::theming::config::limits().max_command_history {
        app.command_history.remove(0);
    }
}

/// `F8` on a typed-line suggestion: forgets `entry` -- every copy, since
/// the suggestions show each command once -- in memory and on disk.
pub(super) fn forget_history(app: &mut App, entry: &str) {
    app.command_history.retain(|recorded| recorded != entry);
    save_history(&app.command_history);
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

/// Substring matches (case-insensitive) for the suggestions that pop up
/// while typing, as in Far: deduplicated, newest first (unlike
/// `matching_history`). An empty query suggests nothing -- `Alt+F8` is the
/// manual search.
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

/// Keys on the History popup: typing filters it live through the command
/// line (as Far's `Alt+F8`); `Up`/`Down` move; `Enter` runs the entry
/// through `submit_command_line`, like a shell's recall; `Tab` copies it
/// into the command line to edit first; `F8` deletes it (memory and disk);
/// `Esc` closes. History: docs/history/command-execution.md.
pub fn handle_history_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if !matches!(app.overlay, Some(Overlay::CommandHistory(_))) {
        return Ok(Effect::None);
    }

    match key.code {
        KeyCode::Up => {
            let Some(Overlay::CommandHistory(menu)) = &mut app.overlay else { unreachable!() };
            crate::list_cursor::move_up(&mut menu.selected);
        }
        KeyCode::Down => {
            let count = matching_history(&app.command_history, app.command_line.text()).len();
            let Some(Overlay::CommandHistory(menu)) = &mut app.overlay else { unreachable!() };
            crate::list_cursor::move_down(&mut menu.selected, count);
        }
        KeyCode::Enter => {
            let Some(Overlay::CommandHistory(menu)) = &app.overlay else { unreachable!() };
            let selected = menu.selected;
            let entry = matching_history(&app.command_history, app.command_line.text()).get(selected).map(|entry| (*entry).clone());
            app.command_line_completion = None;
            app.overlay = None;
            if let Some(entry) = entry {
                app.command_line.set_text(entry);
                return submit_command_line(app);
            }
            app.command_line.move_to_end();
        }
        KeyCode::Tab => {
            let Some(Overlay::CommandHistory(menu)) = &app.overlay else { unreachable!() };
            let selected = menu.selected;
            if let Some(entry) = matching_history(&app.command_history, app.command_line.text()).get(selected) {
                app.command_line.set_text((*entry).clone());
                app.command_line_completion = None;
            }
            app.command_line.move_to_end();
            app.overlay = None;
        }
        KeyCode::F(8) => {
            let Some(Overlay::CommandHistory(menu)) = &app.overlay else { unreachable!() };
            let selected = menu.selected;
            let indices = matching_history_indices(&app.command_history, app.command_line.text());
            if let Some(&index) = indices.get(selected) {
                app.command_history.remove(index);
                save_history(&app.command_history);
            }
            let new_count = matching_history(&app.command_history, app.command_line.text()).len();
            let Some(Overlay::CommandHistory(menu)) = &mut app.overlay else { unreachable!() };
            menu.selected = menu.selected.min(new_count.saturating_sub(1));
        }
        KeyCode::Esc => {
            // This popup edits `app.command_line` as a plain filter, so
            // don't return to a stale mid-line cursor or selection.
            app.command_line.move_to_end();
            app.overlay = None;
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
    if let Some(Overlay::CommandHistory(menu)) = &mut app.overlay {
        menu.selected = 0;
    }
}

#[cfg(test)]
mod tests;
