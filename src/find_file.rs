//! F9 → Commands → Find file: type a filename substring, search
//! recursively from the active panel's directory, jump to whichever
//! result gets picked. Far Manager's own Alt+F7, reached only through
//! the menu here — no global hotkey, that wasn't asked for.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
}


impl FindFileState {
    pub fn new() -> Self {
        Self { phase: FindFilePhase::Typing, query: String::new(), cursor: 0, results: Vec::new(), selected: 0 }
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
            _ => {}
        },
    }

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
    let height = (state.results.len().max(1) as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(70, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(format!(" Find file: \"{}\" ", state.query));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
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

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" go to  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
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
