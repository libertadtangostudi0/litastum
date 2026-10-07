//! `handle_browsing_key`'s behavior and `Alt+F5`'s targets.

mod compare_targets_tests {
    use super::super::compare_targets;
    use crate::test_support::{test_app, unique_scratch_dir};

    #[test]
    fn falls_back_to_the_two_panel_convention_when_nothing_is_marked() {
        let dir = unique_scratch_dir("compare-targets");
        std::fs::write(dir.join("only.txt"), "x").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[0].move_down(); // off ".." and onto "only.txt", in both panels
        app.panels[1].move_down();

        let (left, right) = compare_targets(&app).expect("both panels have a selected file");
        assert_eq!(left, dir.join("only.txt"));
        assert_eq!(right, dir.join("only.txt"), "both panels start on the same directory/selection");
    }

    #[test]
    fn compares_two_marked_entries_in_the_active_panel_instead() {
        let dir = unique_scratch_dir("compare-targets");
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[app.active].select_all();

        let (left, right) = compare_targets(&app).expect("exactly two marked entries");
        assert_eq!(left, dir.join("a.txt"));
        assert_eq!(right, dir.join("b.txt"));
    }

    #[test]
    fn four_marked_conflict_files_open_the_resolver() {
        let dir = unique_scratch_dir("compare-targets");
        crate::conflict::state_tests::write_conflict_files(&dir);
        let mut app = test_app(dir);
        app.panels[app.active].select_all();

        super::super::open_compare(&mut app);

        let crate::app::Mode::ResolveConflict(state) = &app.mode else { panic!("expected the conflict resolver") };
        assert!(state.result.text().contains("<<<<<<<"));
    }

    #[test]
    fn three_marked_entries_falls_back_to_the_two_panel_convention() {
        let dir = unique_scratch_dir("compare-targets");
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        std::fs::write(dir.join("c.txt"), "c").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[app.active].select_all();

        let (left, _right) = compare_targets(&app).expect("both panels have a selected file");
        assert_eq!(left, app.panels[app.active].selected_path().unwrap(), "should have fallen back to the cursor's own selection, not picked two of the three marked entries");
    }
}


/// The browser's key routing -- testable now that terminal work comes
/// back as an `Effect` instead of being done in place.
mod handle_browsing_key_tests {
    use std::fs;

    use crossterm::event::KeyCode;

    use super::super::handle_browsing_key;
    use crate::app::App;
    use crate::command_line::Effect;
    use crate::test_support::{ctrl_key, key, shift_key, test_app, unique_scratch_dir};

    fn typed(line: &str) -> App {
        let mut app = test_app(unique_scratch_dir("browsing-keys"));
        app.command_line.set_text(line);
        app
    }

    fn select_last_chars(app: &mut App, count: usize) {
        for _ in 0..count {
            handle_browsing_key(app, shift_key(KeyCode::Left)).unwrap();
        }
    }

    /// Reported: a selected part of a typed command couldn't be copied.
    #[test]
    fn ctrl_c_copies_the_selected_part_of_the_line() {
        let mut app = typed("svn merge -c 172418,172507 --accept postpone");
        select_last_chars(&mut app, 8);

        handle_browsing_key(&mut app, ctrl_key('c')).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("postpone"));
        assert_eq!(app.command_line.text(), "svn merge -c 172418,172507 --accept postpone", "copying leaves the line alone");
    }

    #[test]
    fn ctrl_insert_copies_too() {
        let mut app = typed("cd src");
        select_last_chars(&mut app, 3);

        handle_browsing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Insert, crossterm::event::KeyModifiers::CONTROL)).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("src"));
    }

    #[test]
    fn ctrl_x_cuts_the_selected_part_of_the_line() {
        let mut app = typed("svn up trunk");
        select_last_chars(&mut app, 5);

        handle_browsing_key(&mut app, ctrl_key('x')).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("trunk"));
        assert_eq!(app.command_line.text(), "svn up ");
    }

    #[test]
    fn ctrl_c_without_a_selection_copies_nothing_and_types_nothing() {
        let mut app = typed("dir");

        handle_browsing_key(&mut app, ctrl_key('c')).unwrap();

        assert_eq!(crate::text_field::clipboard::get(), None);
        assert_eq!(app.command_line.text(), "dir");
    }

    #[test]
    fn ctrl_o_hides_the_panels_and_brings_them_back() {
        let mut app = typed("");
        handle_browsing_key(&mut app, ctrl_key('o')).unwrap();
        assert!(app.panels_hidden);
        handle_browsing_key(&mut app, ctrl_key('o')).unwrap();
        assert!(!app.panels_hidden);
    }

    #[test]
    fn enter_hands_the_typed_line_to_the_shell() {
        let mut app = typed("echo hi");

        let effect = handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(effect, Effect::RunShell(vec!["echo hi".to_string()]));
        assert!(app.command_line.is_empty());
        assert_eq!(app.command_history.last().map(String::as_str), Some("echo hi"));
    }

    #[test]
    fn enter_on_cls_asks_for_a_repaint_not_a_shell() {
        let mut app = typed("cls");
        assert_eq!(handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap(), Effect::ClearScreen);
    }

    #[test]
    fn enter_on_cd_moves_the_panel_without_a_shell() {
        let mut app = typed("cd sub");
        let target = app.panels[app.active].path.join("sub");
        fs::create_dir_all(&target).unwrap();

        assert_eq!(handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap(), Effect::None);
        assert_eq!(app.panels[app.active].path, target);
    }

    /// Over the panels the answer is a notice too, or it'd go unseen.
    #[test]
    fn a_cd_to_a_missing_directory_over_the_panels_shows_a_notice() {
        let mut app = typed("cd no-such-dir");

        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.notice.as_ref().is_some_and(|notice| notice.text.contains("no such directory")));
        assert_eq!(app.user_screen.lines().len(), 2, "and on the user screen, for Ctrl+O");
    }

    #[test]
    fn tab_switches_panels_on_an_empty_line_but_completes_while_typing() {
        let mut app = typed("");
        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.active, 1, "Tab on an empty line switches panels");

        let mut app = typed("cd su");
        fs::create_dir_all(app.panels[app.active].path.join("sub")).unwrap();
        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.active, 0, "Tab while typing must not switch panels");
        assert!(app.command_line.text().starts_with("cd sub"), "completed: {:?}", app.command_line.text());
    }

    #[test]
    fn shift_a_marks_everything_only_on_an_empty_line() {
        let dir = unique_scratch_dir("browsing-keys-shift-a");
        fs::write(dir.join("a.txt"), "a").unwrap();
        let mut app = test_app(dir);

        handle_browsing_key(&mut app, shift_key(KeyCode::Char('A'))).unwrap();
        assert!(!app.panels[app.active].marked_entries().is_empty(), "empty line: Shift+A marks all");
        assert!(app.command_line.is_empty());

        app.command_line.set_text("x");
        handle_browsing_key(&mut app, shift_key(KeyCode::Char('A'))).unwrap();
        assert_eq!(app.command_line.text(), "xA", "while typing, Shift+A is just a capital letter");
    }

    #[test]
    fn a_plain_letter_types_instead_of_acting_as_a_shortcut() {
        let mut app = typed("");
        handle_browsing_key(&mut app, key(KeyCode::Char('q'))).unwrap();
        assert_eq!(app.command_line.text(), "q");
        assert!(!app.should_quit, "only F10 quits; a bare q is the start of a command");
    }

    /// Reported: a mistyped command kept being suggested, with no way to
    /// get rid of it.
    #[test]
    fn f8_forgets_the_highlighted_suggestion_and_deletes_no_file() {
        let dir = unique_scratch_dir("browsing-keys-f8");
        fs::write(dir.join("keep.txt"), "x").unwrap();
        let mut app = test_app(dir.clone());
        app.command_history = vec!["svn up".into(), "svn info".into(), "svn up".into(), "cargo build".into()];
        app.command_line.set_text("svn");
        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap(); // newest first: "svn up", then "svn info"

        handle_browsing_key(&mut app, key(KeyCode::F(8))).unwrap();

        assert_eq!(app.command_history, vec!["svn up".to_string(), "svn up".into(), "cargo build".into()]);
        assert_eq!(app.command_line.text(), "svn", "the typed line stays");
        assert_eq!(app.command_line_suggestion_selected, 0, "clamped to the one suggestion left");
        assert!(app.overlay.is_none(), "no delete confirmation");
        assert!(dir.join("keep.txt").exists());

        handle_browsing_key(&mut app, key(KeyCode::F(8))).unwrap();
        assert_eq!(app.command_history, vec!["cargo build".to_string()], "every copy of the command goes");
    }

    /// Reported: a file name couldn't be completed from the suggestions.
    #[test]
    fn tab_accepts_a_panel_name_into_the_typed_word_and_f8_leaves_files_alone() {
        let dir = unique_scratch_dir("browsing-keys-names");
        fs::write(dir.join("cmt_msg.txt"), "x").unwrap();
        let mut app = test_app(dir.clone());
        app.command_line.set_text("svn commit -F cm");

        handle_browsing_key(&mut app, key(KeyCode::F(8))).unwrap();
        assert!(dir.join("cmt_msg.txt").exists(), "F8 on a file suggestion deletes nothing");
        assert!(app.overlay.is_none());

        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.command_line.text(), "svn commit -F cmt_msg.txt ");
    }

    /// Requested: F4 on a panel name in the suggestions moves the panel's
    /// cursor onto that file and opens it, as in the path field's list.
    #[test]
    fn f4_on_a_file_suggestion_selects_it_in_the_panel_and_opens_the_editor() {
        let dir = unique_scratch_dir("browsing-keys-f4");
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("cmt_msg.txt"), "message\n").unwrap();
        let mut app = test_app(dir.clone());
        app.command_line.set_text("svn commit -F cm");

        handle_browsing_key(&mut app, key(KeyCode::F(4))).unwrap();

        assert_eq!(app.panels[app.active].current().map(|entry| entry.name.as_str()), Some("cmt_msg.txt"));
        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("cmt_msg.txt"));
        assert_eq!(app.command_line.text(), "svn commit -F cm", "the typed line stays");
    }

    /// Reported: with `cmt_msg.txt` also in the history, its row was a
    /// history entry and F4 didn't open the file.
    #[test]
    fn f4_opens_a_file_whose_name_is_also_in_the_history() {
        let dir = unique_scratch_dir("browsing-keys-f4");
        fs::write(dir.join("cmt_msg.txt"), "message
").unwrap();
        let mut app = test_app(dir.clone());
        app.command_history = vec!["cmt_msg.txt".into(), "svn commit -F cmt_msg.txt RFI14.1".into()];
        app.command_line.set_text("cmt");
        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap(); // newest history first, then the name

        handle_browsing_key(&mut app, key(KeyCode::F(4))).unwrap();

        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("cmt_msg.txt"));
    }

    #[test]
    fn shift_left_selects_in_the_typed_line() {
        let mut app = typed("abc");
        handle_browsing_key(&mut app, shift_key(KeyCode::Left)).unwrap();
        assert_eq!(app.command_line.selection(), Some((2, 3)));
    }
}
