//! F9 → Commands → Find file (also `Alt+F7`, `command_line.rs`): type
//! a filename substring or glob (`*`/`?`), search recursively from the
//! active panel's directory, jump to whichever result gets picked, or
//! `Ctrl+S` to export the full list to a file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use directories::UserDirs;
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use tracing::debug;

use crate::app::{App, Mode};
use crate::text_field;
use crate::theme::Theme;
use crate::ui::centered_rect;

/// Search results are capped, and the walk itself gives up after
/// visiting this many entries — a huge tree (a repo's own `target/`,
/// `.git/`, `node_modules/`, ...) shouldn't be able to hang the UI
/// indefinitely. Deliberately no directory exclusions beyond that cap:
/// a plain substring search, same starting point as Far Manager's own
/// "Find file" before its own filter options are touched.
const MAX_RESULTS: usize = 200;
const MAX_VISITED: usize = 50_000;


/// Which half of the popup is showing — the typed query, or the
/// results it produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindFilePhase {
    Typing,
    Results,
}


pub struct FindFileState {
    pub phase: FindFilePhase,
    pub query: String,
    /// Character index into `query` — see `text_field.rs`.
    pub cursor: usize,
    pub results: Vec<PathBuf>,
    pub selected: usize,
    /// Feedback from the last `Ctrl+S` export, shown under the results
    /// list until the next export attempt (success or failure — both
    /// get a message, there's no other status-bar surface to put this
    /// on yet). `(label, detail)` — `("Exported to:", "<path>")` or
    /// `("Export failed:", "<error>")` — rendered on two separate
    /// lines rather than one, since a real Downloads path is easily
    /// wide enough to blow past the popup's width on one line.
    pub export_message: Option<(String, String)>,
}


impl FindFileState {
    pub fn new() -> Self {
        Self {
            phase: FindFilePhase::Typing,
            query: String::new(),
            cursor: 0,
            results: Vec::new(),
            selected: 0,
            export_message: None,
        }
    }
}


/// Recursively searches `root` for entries whose file name matches
/// `query` (case-insensitively), returning matching paths in the order
/// found (a plain `fs::read_dir` walk order, not sorted — good enough
/// for a first pass at this feature). See `MAX_RESULTS`/`MAX_VISITED`
/// for the safety caps.
///
/// `query` is a glob pattern (`*`/`?`, Far Manager's own convention for
/// this dialog — `*.md`, `read?e.txt`) if it contains either wildcard
/// character; otherwise it's a plain substring, which covers the
/// common "just type part of the name" case without forcing `*name*`
/// on every query. Found missing by hand: `*.md` was searched for
/// *literally* (as the six-character substring `"*.md"`, which no real
/// file name contains) before this distinction existed.
pub fn search(root: &Path, query: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut visited = 0;
    let query_lower = query.to_lowercase();
    search_into(root, &query_lower, &mut results, &mut visited);
    results
}


fn search_into(dir: &Path, query_lower: &str, results: &mut Vec<PathBuf>, visited: &mut usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(|entry| entry.ok()) {
        if results.len() >= MAX_RESULTS || *visited >= MAX_VISITED {
            return;
        }
        *visited += 1;

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if matches_query(&name, query_lower) {
            results.push(path.clone());
        }

        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            search_into(&path, query_lower, results, visited);
        }
    }
}


/// `name` and `query_lower` are both assumed already lowercased by the
/// caller (`search`/`search_into`). Glob semantics only kick in once
/// `query_lower` actually contains a wildcard character.
fn matches_query(name: &str, query_lower: &str) -> bool {
    if query_lower.contains('*') || query_lower.contains('?') {
        glob_match(query_lower, name)
    } else {
        name.contains(query_lower)
    }
}


/// Classic greedy `*`/`?` wildcard matching (`*` — any run of
/// characters, including none; `?` — exactly one character) — the
/// textbook two-pointer-plus-backtrack-point algorithm, not a
/// full-featured glob (no `[...]` character classes, no escaping).
/// Operates on `char`s rather than bytes so a multi-byte file name
/// can't be split mid-character.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    // Where the most recent unresolved `*` sits in `pattern`, and how
    // far into `text` we've tried stretching it to cover so far --
    // `None` until the first `*` is seen, since there's nothing to
    // backtrack to before that.
    let mut star_p: Option<usize> = None;
    let mut star_t = 0;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star_p = Some(p);
            star_t = t;
            p += 1;
        } else if let Some(sp) = star_p {
            // The match after the last `*` failed -- stretch that `*`
            // to cover one more character and retry from right after it.
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|&c| c == '*')
}


/// Finds the user's Downloads directory and hands off to
/// `write_results` — `Ctrl+S` on the results popup, requested
/// explicitly (Far Manager itself has no equivalent of this). Split
/// from `write_results` so the actual file-writing logic is testable
/// against a scratch directory rather than the real, un-injectable
/// Downloads path (same reasoning as `config.rs`'s `set_interface_theme`
/// vs. `try_persist`).
fn export_results(state: &FindFileState) -> io::Result<PathBuf> {
    let downloads = UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Downloads directory available on this platform"))?;
    write_results(&downloads, state)
}


/// Writes `state.results` (one full path per line) to a new file in
/// `dir` and returns the path written. The file name embeds the search
/// query and a timestamp (`find-results_<query>_<timestamp>.txt`) so
/// repeated exports for different searches — or the same one, run
/// again later — don't overwrite each other.
fn write_results(dir: &Path, state: &FindFileState) -> io::Result<PathBuf> {
    let filename = format!("find-results_{}_{}.txt", sanitize_for_filename(&state.query), timestamp_for_filename());
    let path = dir.join(filename);

    let mut contents = String::new();
    for result in &state.results {
        contents.push_str(&result.display().to_string());
        contents.push('\n');
    }
    fs::write(&path, contents)?;
    Ok(path)
}


/// Replaces every character Windows (the strictest common case) won't
/// allow in a file name with `_`, so an arbitrary search query is
/// always safe to embed in `export_results`'s file name — falls back
/// to a fixed placeholder if that leaves nothing at all (an
/// all-wildcard query like `"***"` sanitizes to `"___"`, which is at
/// least non-empty, but an empty query itself would otherwise produce
/// a file name with two consecutive underscores and no query in it).
fn sanitize_for_filename(query: &str) -> String {
    let cleaned: String =
        query.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    if cleaned.is_empty() {
        "query".to_string()
    } else {
        cleaned
    }
}


/// `YYYY-MM-DD_HHMMSS`, computed by hand from `SystemTime` (UTC, not
/// local time — no timezone lookup this way, but this is only ever
/// used to keep export file names from colliding with each other, not
/// as a user-facing display of "when") rather than pulling in a
/// date/time crate for one file-name timestamp. The days-to-civil-date
/// conversion is Howard Hinnant's well-known public-domain algorithm
/// (<http://howardhinnant.github.io/date_algorithms.html>), not
/// something invented here.
fn timestamp_for_filename() -> String {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = since_epoch.as_secs();
    let (days, secs_of_day) = (total_secs / 86400, total_secs % 86400);
    let (hour, minute, second) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}_{hour:02}{minute:02}{second:02}")
}


fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let day_of_era = (z - era * 146097) as u64;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}


/// Key handling for both phases of the popup: typing the query
/// (full-cursor editing, `text_field.rs` — same reasoning as the F5/F6
/// transfer prompt, this is a modal popup with no panel navigation
/// happening under it) and, once `Enter` runs a search, picking a
/// result with `Up`/`Down`/`Enter`. `Esc` closes from either phase.
pub fn handle_find_file_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        app.mode = Mode::Browsing;
        return Ok(());
    }

    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };

    match state.phase {
        FindFilePhase::Typing => {
            if key.code == KeyCode::Enter {
                return run_search(app);
            }

            let Mode::FindFile(state) = &mut app.mode else {
                unreachable!("just matched Mode::FindFile above");
            };
            match key.code {
                KeyCode::Backspace => text_field::backspace(&mut state.query, &mut state.cursor),
                KeyCode::Left => text_field::move_left(&mut state.cursor),
                KeyCode::Right => text_field::move_right(&state.query, &mut state.cursor),
                KeyCode::Home => text_field::move_home(&mut state.cursor),
                KeyCode::End => text_field::move_end(&state.query, &mut state.cursor),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text_field::insert_char(&mut state.query, &mut state.cursor, c);
                }
                _ => {}
            }
        }
        FindFilePhase::Results => match key.code {
            KeyCode::Up => {
                let Mode::FindFile(state) = &mut app.mode else {
                    unreachable!("just matched Mode::FindFile above");
                };
                state.selected = state.selected.saturating_sub(1);
            }
            KeyCode::Down => {
                let Mode::FindFile(state) = &mut app.mode else {
                    unreachable!("just matched Mode::FindFile above");
                };
                if state.selected + 1 < state.results.len() {
                    state.selected += 1;
                }
            }
            KeyCode::Enter => return open_selected_result(app),
            KeyCode::Char('s' | 'S') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return run_export(app);
            }
            _ => {}
        },
    }

    Ok(())
}


/// `Ctrl+S` on the results popup: writes `export_results` and records
/// the outcome (success or failure, both — there's no other
/// status-bar surface to report a failure on yet) in
/// `state.export_message` for `draw_results` to show.
fn run_export(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let message = match export_results(state) {
        Ok(path) => {
            debug!(path = %path.display(), "find file: exported results");
            ("Exported to:".to_string(), path.display().to_string())
        }
        Err(err) => {
            debug!(%err, "find file: export failed");
            ("Export failed:".to_string(), err.to_string())
        }
    };

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("just matched Mode::FindFile above");
    };
    state.export_message = Some(message);
    Ok(())
}


/// `Enter` while typing: runs `search` from the active panel's
/// directory and switches to `FindFilePhase::Results`. A no-op on an
/// empty query (nothing sensible to search for).
fn run_search(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    if state.query.is_empty() {
        return Ok(());
    }
    let query = state.query.clone();
    let root = app.panels[app.active].path.clone();
    debug!(query, root = %root.display(), "find file: searching");
    let results = search(&root, &query);
    debug!(count = results.len(), "find file: search finished");

    let Mode::FindFile(state) = &mut app.mode else {
        unreachable!("just matched Mode::FindFile above");
    };
    state.results = results;
    state.selected = 0;
    state.phase = FindFilePhase::Results;
    state.export_message = None; // a stale message from a previous search shouldn't linger
    Ok(())
}


/// `Enter` on a result: closes the popup and moves the active panel to
/// the result's directory with it selected, same as double-clicking a
/// search hit in a real file manager would.
fn open_selected_result(app: &mut App) -> Result<()> {
    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };
    let Some(path) = state.results.get(state.selected).cloned() else {
        return Ok(());
    };

    app.mode = Mode::Browsing;

    let Some(parent) = path.parent() else {
        return Ok(());
    };
    app.panels[app.active].path = parent.to_path_buf();
    app.panels[app.active].reload()?;
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if let Some(index) = app.panels[app.active].entries.iter().position(|entry| entry.name == name) {
            app.panels[app.active].selected = index;
        }
    }
    Ok(())
}


/// Renders whichever phase is current. Returns where the real terminal
/// cursor should sit — only meaningful during `Typing` (same mechanism
/// as the command line's own cursor, `ui::draw`); `None` during
/// `Results`, which has no text entry to place a cursor in.
pub fn draw_find_file(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) -> Option<Position> {
    match state.phase {
        FindFilePhase::Typing => Some(draw_typing(frame, area, state, theme)),
        FindFilePhase::Results => {
            draw_results(frame, area, state, theme);
            None
        }
    }
}


fn draw_typing(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) -> Position {
    let popup = centered_rect(50, 5, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Find file ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let label = Line::from(Span::styled("File name to find:", Style::default().fg(theme.text)));
    frame.render_widget(label, rows[0]);

    let query = Line::from(Span::styled(state.query.clone(), Style::default().fg(theme.text)));
    frame.render_widget(query, rows[1]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" search  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[2]);

    Position { x: rows[1].x + state.cursor as u16, y: rows[1].y }
}


fn draw_results(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) {
    let height = (state.results.len().max(1) as u16 + 6).clamp(8, area.height);
    let popup = centered_rect(70, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(format!(" Find file: \"{}\" ", state.query));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Two fixed rows for the export message (label + detail, e.g.
    // "Exported to:" / the actual path) even when there isn't one --
    // simpler than resizing the popup depending on whether a message
    // is currently showing, at the cost of a little blank space in the
    // common "haven't exported yet" case.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    if state.results.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled("No matches found", Style::default().fg(theme.text_dim))));
        frame.render_widget(empty, rows[0]);
    } else {
        let items: Vec<ListItem> = state
            .results
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let style = if index == state.selected {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(path.to_string_lossy().into_owned(), style)))
            })
            .collect();
        frame.render_widget(List::new(items), rows[0]);
    }

    if let Some((label, detail)) = &state.export_message {
        let label_line = Line::from(Span::styled(label.clone(), Style::default().fg(theme.text_dim)));
        frame.render_widget(label_line, rows[1]);
        // A real Downloads path is easily wide enough to overflow the
        // popup on one line together with the label -- ratatui clips
        // rather than wraps a `Line` that's too long for its area, so
        // splitting the detail onto its own row (still just clipped if
        // it's *itself* wider than the popup, but that's a much rarer
        // case than "label + path together" was) is the fix here, not
        // a text-wrapping widget for what's meant to be a one-line
        // status.
        let detail_line = Line::from(Span::styled(detail.clone(), Style::default().fg(theme.text)));
        frame.render_widget(detail_line, rows[2]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" go to  ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+S", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" export  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[3]);
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::theme::Theme;

    fn scratch_dir() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litastum-find-file-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn search_finds_a_matching_file_at_the_root() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();
        fs::write(dir.join("other.txt"), b"hi").unwrap();

        let results = search(&dir, "read");

        assert_eq!(results, vec![dir.join("readme.txt")]);
    }

    #[test]
    fn search_matches_case_insensitively() {
        let dir = scratch_dir();
        fs::write(dir.join("README.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "read"), vec![dir.join("README.txt")]);
    }

    #[test]
    fn search_descends_into_subdirectories() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested").join("target.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "target"), vec![dir.join("nested").join("target.txt")]);
    }

    #[test]
    fn search_matches_directory_names_too() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("target_dir")).unwrap();

        assert_eq!(search(&dir, "target"), vec![dir.join("target_dir")]);
    }

    #[test]
    fn search_with_no_matches_returns_empty() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        assert!(search(&dir, "nope").is_empty());
    }

    /// Regression test for the actual reported bug: `*.md` used to be
    /// searched for as the literal six-character substring `"*.md"`,
    /// which matches nothing real, instead of as a glob pattern.
    #[test]
    fn search_treats_a_star_pattern_as_a_glob_not_a_literal_substring() {
        let dir = scratch_dir();
        fs::write(dir.join("README.md"), b"hi").unwrap();
        fs::write(dir.join("notes.txt"), b"hi").unwrap();

        let results = search(&dir, "*.md");

        assert_eq!(results, vec![dir.join("README.md")]);
    }

    #[test]
    fn search_glob_question_mark_matches_exactly_one_character() {
        let dir = scratch_dir();
        fs::write(dir.join("cat.txt"), b"hi").unwrap();
        fs::write(dir.join("cats.txt"), b"hi").unwrap();

        let results = search(&dir, "ca?.txt");

        assert_eq!(results, vec![dir.join("cat.txt")], "should match exactly one character, not \"cats\"'s two");
    }

    #[test]
    fn search_glob_star_can_match_the_empty_string() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();

        assert_eq!(search(&dir, "readme*.txt"), vec![dir.join("readme.txt")]);
    }

    #[test]
    fn glob_match_examples() {
        assert!(glob_match("*.md", "readme.md"));
        assert!(!glob_match("*.md", "readme.txt"));
        assert!(glob_match("read?e.txt", "readme.txt"));
        assert!(!glob_match("read?e.txt", "readmme.txt"), "? is exactly one character, not one-or-more");
        assert!(glob_match("*", "anything.at.all"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"), "missing the trailing c");
        assert!(glob_match("", ""));
        assert!(!glob_match("a", ""));
        assert!(glob_match("*", ""), "a bare * matches even an empty string");
    }

    #[test]
    fn sanitize_for_filename_replaces_windows_illegal_characters() {
        assert_eq!(sanitize_for_filename("*.md"), "_.md");
        assert_eq!(sanitize_for_filename("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_for_filename("normal-name"), "normal-name", "nothing to change here");
    }

    #[test]
    fn sanitize_for_filename_falls_back_to_a_placeholder_when_nothing_is_left() {
        assert_eq!(sanitize_for_filename(""), "query");
    }

    #[test]
    fn sanitize_for_filename_an_all_wildcard_query_stays_non_empty() {
        // "***" sanitizes to "___" -- non-empty, so the "query"
        // fallback (for a literally empty input) doesn't kick in here.
        assert_eq!(sanitize_for_filename("***"), "___");
    }

    #[test]
    fn civil_from_days_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_end_of_january_1970() {
        assert_eq!(civil_from_days(30), (1970, 1, 31));
        assert_eq!(civil_from_days(31), (1970, 2, 1));
    }

    #[test]
    fn civil_from_days_end_of_a_non_leap_february() {
        // 1970 is not a leap year: Jan (31, indices 0..30) + Feb (28,
        // indices 31..58) -- index 59 is the first day of March.
        assert_eq!(civil_from_days(58), (1970, 2, 28));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
    }

    #[test]
    fn civil_from_days_new_year_rollovers() {
        assert_eq!(civil_from_days(365), (1971, 1, 1), "1970 has 365 days, not a leap year");
        assert_eq!(civil_from_days(365 + 365), (1972, 1, 1), "1971 also not a leap year");
    }

    #[test]
    fn timestamp_for_filename_has_the_expected_shape() {
        let timestamp = timestamp_for_filename();
        assert_eq!(timestamp.len(), 17, "YYYY-MM-DD_HHMMSS: {timestamp}");
        assert_eq!(timestamp.as_bytes()[4], b'-');
        assert_eq!(timestamp.as_bytes()[7], b'-');
        assert_eq!(timestamp.as_bytes()[10], b'_');
        assert!(
            timestamp.chars().enumerate().all(|(i, c)| [4, 7, 10].contains(&i) || c.is_ascii_digit()),
            "everything but the separators should be digits: {timestamp}"
        );
    }

    #[test]
    fn write_results_writes_one_path_per_line() {
        let dir = scratch_dir();
        let mut state = FindFileState::new();
        state.query = "test".to_string();
        state.results = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];

        let path = write_results(&dir, &state).expect("export should succeed");

        assert!(path.starts_with(&dir));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a.txt\nb.txt\n");
    }

    #[test]
    fn write_results_with_no_results_writes_an_empty_file() {
        let dir = scratch_dir();
        let state = FindFileState::new();

        let path = write_results(&dir, &state).expect("export should succeed even with nothing found");

        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn write_results_filename_embeds_the_sanitized_query() {
        let dir = scratch_dir();
        let mut state = FindFileState::new();
        state.query = "*.md".to_string();

        let path = write_results(&dir, &state).unwrap();

        let filename = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(filename.starts_with("find-results_"), "{filename}");
        assert!(filename.contains("_.md_"), "sanitized '*.md' should appear between the prefix and the timestamp: {filename}");
        assert!(filename.ends_with(".txt"), "{filename}");
    }

    // `run_export`/`Ctrl+S` dispatch itself isn't exercised end to end
    // here: `export_results` goes through the real, un-injectable
    // Downloads directory (see its own doc comment), so calling it
    // would write into the real user's Downloads folder as a side
    // effect of running the test suite. `write_results` above covers
    // everything export-related that's actually injectable.

    fn app_with_find_file(state: FindFileState) -> App {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litastum-find-file-app-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        let mut app = App::new(dir, Theme::dark(), None).expect("build app");
        app.mode = Mode::FindFile(state);
        app
    }

    #[test]
    fn typing_inserts_into_the_query() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_find_file_key(&mut app, key(KeyCode::Char('a'))).unwrap();
        handle_find_file_key(&mut app, key(KeyCode::Char('b'))).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.query, "ab");
    }

    #[test]
    fn enter_on_an_empty_query_does_not_search() {
        let mut app = app_with_find_file(FindFileState::new());

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Typing);
    }

    #[test]
    fn enter_on_a_real_query_runs_a_search_and_switches_to_results() {
        let mut state = FindFileState::new();
        state.query = "sou".to_string();
        state.cursor = 3;
        let mut app = app_with_find_file(state);
        fs::write(app.panels[0].path.join("source.txt"), b"hi").unwrap();

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.phase, FindFilePhase::Results);
        assert_eq!(state.results, vec![app.panels[0].path.join("source.txt")]);
    }

    #[test]
    fn esc_closes_from_either_phase() {
        let mut app = app_with_find_file(FindFileState::new());
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing));

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        let mut app = app_with_find_file(results_state);
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn enter_on_a_result_navigates_the_active_panel_and_selects_it() {
        let mut app = app_with_find_file(FindFileState::new());
        let target_dir = app.panels[0].path.join("nested");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("target.txt"), b"hi").unwrap();

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![target_dir.join("target.txt")];
        app.mode = Mode::FindFile(results_state);

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert_eq!(app.panels[0].path, target_dir);
        let selected_name = &app.panels[0].entries[app.panels[0].selected].name;
        assert_eq!(selected_name, "target.txt");
    }

    #[test]
    fn up_and_down_move_the_result_selection() {
        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        results_state.results = vec![PathBuf::from("a"), PathBuf::from("b")];
        let mut app = app_with_find_file(results_state);

        handle_find_file_key(&mut app, key(KeyCode::Down)).unwrap();

        let Mode::FindFile(state) = &app.mode else { panic!("expected Mode::FindFile") };
        assert_eq!(state.selected, 1);
    }
}
