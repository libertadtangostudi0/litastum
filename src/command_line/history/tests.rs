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
        assert_eq!(app.command_line.text(), "", "run_command_line clears the line once it's actually run");
        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_history_key_esc_cancels_without_changing_the_command_line() {
        let mut app = app_in_history_menu(vec!["dir"]);
        app.command_line.set_text("untouched");

        handle_history_key(&mut app, key(KeyCode::Esc), &mut dummy_terminal()).unwrap();

        assert_eq!(app.command_line.text(), "untouched");
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
        assert_eq!(app.command_line.text(), "");
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

        assert_eq!(app.command_line.text(), "svn");
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

        assert_eq!(app.command_line.text(), "cd nonexistent-dir");
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
