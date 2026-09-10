use super::*;

mod editing_tests {
    use super::*;

    #[test]
    fn insert_char_at_cursor_not_just_at_the_end() {
        let mut text = "helo".to_string();
        let mut cursor = 3; // "hel|o"
        insert_char(&mut text, &mut cursor, 'l');
        assert_eq!(text, "hello");
        assert_eq!(cursor, 4);
    }

    #[test]
    fn insert_char_handles_multibyte_utf8() {
        let mut text = "café".to_string();
        let mut cursor = 4; // after the 4th char ('é'), the end
        insert_char(&mut text, &mut cursor, '!');
        assert_eq!(text, "café!");
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        let mut text = "hello".to_string();
        let mut cursor = 3; // "hel|lo"
        backspace(&mut text, &mut cursor);
        assert_eq!(text, "helo");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn backspace_at_start_is_a_noop() {
        let mut text = "hello".to_string();
        let mut cursor = 0;
        backspace(&mut text, &mut cursor);
        assert_eq!(text, "hello");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn delete_forward_removes_the_character_at_the_cursor_without_moving_it() {
        let mut text = "hello".to_string();
        let mut cursor = 1; // "h|ello"
        delete_forward(&mut text, &mut cursor);
        assert_eq!(text, "hllo");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn delete_forward_at_end_is_a_noop() {
        let mut text = "hello".to_string();
        let mut cursor = 5;
        delete_forward(&mut text, &mut cursor);
        assert_eq!(text, "hello");
    }
}

mod movement_tests {
    use super::*;

    #[test]
    fn move_left_and_right_are_clamped() {
        let text = "ab";
        let mut cursor = 0;
        move_left(&mut cursor);
        assert_eq!(cursor, 0, "clamped at start");

        cursor = 2;
        move_right(text, &mut cursor);
        assert_eq!(cursor, 2, "clamped at end");
    }

    #[test]
    fn home_and_end_jump_to_the_edges() {
        let text = "hello";
        let mut cursor = 2;
        move_home(&mut cursor);
        assert_eq!(cursor, 0);
        move_end(text, &mut cursor);
        assert_eq!(cursor, 5);
    }

    #[test]
    fn move_word_left_skips_separators_then_the_word() {
        let text = "C:/Users/name/file.txt";
        let mut cursor = text.chars().count(); // end, right after "file.txt"
        move_word_left(text, &mut cursor);
        // Lands at the start of "txt" -- '.' isn't a word char, so it's
        // skipped as a separator, same as a real editor's Ctrl+Left.
        assert_eq!(&text[cursor..], "txt");
    }

    #[test]
    fn move_word_right_skips_the_word_then_separators() {
        let text = "name/file";
        let mut cursor = 0;
        move_word_right(text, &mut cursor);
        assert_eq!(&text[cursor..], "/file");
    }

    /// Regression test for the real reported path: crossing a `/`
    /// needs its own Ctrl+Right press, not silently chained into the
    /// following path segment the way a space or `.` would be.
    #[test]
    fn move_word_right_stops_right_after_a_path_separator() {
        let text = "/branches/features";
        let mut cursor = 0;

        move_word_right(text, &mut cursor); // over the leading "/"
        assert_eq!(cursor, 1);

        move_word_right(text, &mut cursor); // over "branches"
        assert_eq!(&text[cursor..], "/features");

        move_word_right(text, &mut cursor); // over that "/"
        assert_eq!(&text[cursor..], "features");
    }

    #[test]
    fn move_word_left_stops_right_before_a_path_separator() {
        let text = "/branches/features";
        let mut cursor = text.chars().count();

        move_word_left(text, &mut cursor); // back over "features"
        assert_eq!(&text[cursor..], "features");

        move_word_left(text, &mut cursor); // back over that "/"
        assert_eq!(&text[cursor..], "/features");

        move_word_left(text, &mut cursor); // back over "branches"
        assert_eq!(&text[cursor..], "branches/features");

        move_word_left(text, &mut cursor); // back over the leading "/"
        assert_eq!(cursor, 0);
    }

    /// Backslashes (Windows paths) get the same per-character stop as
    /// forward slashes.
    #[test]
    fn move_word_right_stops_right_after_a_backslash() {
        let text = r"C:\Users\name";
        let mut cursor = 2; // right after "C:"

        move_word_right(text, &mut cursor); // over the "\"
        assert_eq!(cursor, 3);
    }

    #[test]
    fn word_movement_is_clamped_at_the_edges() {
        let text = "word";
        let mut cursor = 0;
        move_word_left(text, &mut cursor);
        assert_eq!(cursor, 0);

        cursor = text.chars().count();
        move_word_right(text, &mut cursor);
        assert_eq!(cursor, text.chars().count());
    }
}

mod selection_tests {
    use super::*;

    #[test]
    fn shift_right_starts_and_extends_a_selection() {
        let text = "hello";
        let mut cursor = 1;
        let mut anchor = None;
        extend_selection_right(text, &mut cursor, &mut anchor);
        extend_selection_right(text, &mut cursor, &mut anchor);
        assert_eq!(anchor, Some(1));
        assert_eq!(cursor, 3);
        assert_eq!(selection_range(anchor.unwrap(), cursor), (1, 3));
    }

    #[test]
    fn shift_left_from_a_shift_right_selection_shrinks_it_back() {
        // Selecting right then left past the anchor should flip which
        // side is the selection start -- selection_range must still
        // normalize it, not assume anchor <= cursor.
        let text = "hello";
        let mut cursor = 2;
        let mut anchor = None;
        extend_selection_right(text, &mut cursor, &mut anchor);
        extend_selection_left(&mut cursor, &mut anchor);
        extend_selection_left(&mut cursor, &mut anchor);
        assert_eq!(selection_range(anchor.unwrap(), cursor), (1, 2));
    }

    #[test]
    fn ctrl_shift_right_extends_selection_by_a_whole_word() {
        let text = "svn info";
        let mut cursor = 0;
        let mut anchor = None;
        extend_selection_word_right(text, &mut cursor, &mut anchor);
        assert_eq!(anchor, Some(0));
        assert_eq!(cursor, 3, "should land right after \"svn\", before the space");
        assert_eq!(selection_range(anchor.unwrap(), cursor), (0, 3));
    }

    #[test]
    fn ctrl_shift_left_extends_selection_by_a_whole_word() {
        let text = "svn info";
        let mut cursor = 8; // end
        let mut anchor = None;
        extend_selection_word_left(text, &mut cursor, &mut anchor);
        assert_eq!(anchor, Some(8));
        assert_eq!(cursor, 4, "should land right at the start of \"info\"");
    }

    #[test]
    fn plain_left_collapses_selection_to_its_start() {
        let mut cursor = 4;
        let mut anchor = Some(1);
        collapse_selection_left(&mut cursor, &mut anchor);
        assert_eq!(cursor, 1);
        assert_eq!(anchor, None, "selection should be cleared, not just moved");
    }

    #[test]
    fn plain_right_collapses_selection_to_its_end() {
        let text = "hello";
        let mut cursor = 1;
        let mut anchor = Some(4);
        collapse_selection_right(text, &mut cursor, &mut anchor);
        assert_eq!(cursor, 4);
        assert_eq!(anchor, None);
    }

    #[test]
    fn plain_left_with_no_selection_just_moves_left() {
        let mut cursor = 2;
        let mut anchor = None;
        collapse_selection_left(&mut cursor, &mut anchor);
        assert_eq!(cursor, 1);
    }

    #[test]
    fn delete_selection_removes_the_selected_range_and_clears_it() {
        let mut text = "hello world".to_string();
        let mut cursor = 5; // "hello| world" -- selected "hello" via anchor 0
        let mut anchor = Some(0);
        let deleted = delete_selection(&mut text, &mut cursor, &mut anchor);
        assert!(deleted);
        assert_eq!(text, " world");
        assert_eq!(cursor, 0);
        assert_eq!(anchor, None);
    }

    #[test]
    fn delete_selection_is_a_noop_and_returns_false_with_nothing_selected() {
        let mut text = "hello".to_string();
        let mut cursor = 2;
        let mut anchor = None;
        let deleted = delete_selection(&mut text, &mut cursor, &mut anchor);
        assert!(!deleted);
        assert_eq!(text, "hello");
        assert_eq!(cursor, 2);
    }
}
