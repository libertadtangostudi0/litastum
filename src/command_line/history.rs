use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Mode};

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
pub fn record_history(app: &mut App, input: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_history(history: Vec<&str>) -> App {
        let mut app = test_app(unique_scratch_dir("command-line-history"));
        app.command_history = history.into_iter().map(String::from).collect();
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
            let mut app = app_with_history((0..MAX_HISTORY).map(|_| "placeholder").collect());
            // Break up the run of identical "placeholder" entries first, or
            // the immediate-repeat skip above would swallow the new one.
            record_history(&mut app, "distinct");
            assert_eq!(app.command_history.len(), MAX_HISTORY);
            assert_eq!(app.command_history.last().unwrap(), "distinct");
        }
    }

    mod history_key_handling_tests {
        use super::*;

        fn app_in_history_menu(history: Vec<&str>) -> App {
            let mut app = app_with_history(history);
            app.mode = Mode::CommandHistory(CommandHistoryMenu::open());
            app
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
}
