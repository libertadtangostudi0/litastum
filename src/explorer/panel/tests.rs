use super::*;
use crate::test_support::unique_scratch_dir;

/// A panel with `count` dummy file entries, laid out in `columns`
/// columns, cursor starting at index 0. Shared by `navigation_tests`
/// and `scrolling_tests` below.
fn panel_with(count: usize, columns: usize) -> Panel {
    let entries = (0..count)
        .map(|i| Entry {
            name: i.to_string(),
            is_dir: false,
            size: 0,
            modified: None,
        })
        .collect();
    Panel {
        path: PathBuf::new(),
        entries,
        selected: 0,
        columns,
        scroll_offset: 0,
        visible_rows: 0,
        marked: HashSet::new(),
    }
}

mod change_dir_tests {
    use super::*;

    /// A real scratch directory with one real subdirectory (`sub`) in
    /// it, for `change_dir` tests — unlike `panel_with` below, this
    /// needs actual filesystem entries since `change_dir` checks
    /// `is_dir()` and calls `reload()`.
    fn scratch_panel() -> Panel {
        let dir = unique_scratch_dir("panel");
        fs::create_dir_all(dir.join("sub")).expect("create scratch dirs");
        Panel::new(dir).expect("open scratch panel")
    }

    #[test]
    fn change_dir_descends_into_a_real_subdirectory() {
        let mut panel = scratch_panel();
        let original = panel.path.clone();

        panel.change_dir("sub").unwrap();

        assert_eq!(panel.path, original.join("sub"));
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn change_dir_ignores_a_target_that_is_not_a_directory() {
        let mut panel = scratch_panel();
        let original = panel.path.clone();

        panel.change_dir("does-not-exist").unwrap();

        assert_eq!(panel.path, original, "path should be unchanged");
    }

    #[test]
    fn change_dir_up_via_dotdot() {
        let mut panel = scratch_panel();
        let original = panel.path.clone();
        panel.change_dir("sub").unwrap();

        panel.change_dir("..").unwrap();

        assert_eq!(panel.path, original);
    }

    #[test]
    fn lexically_normalize_collapses_parent_dir_segments() {
        assert_eq!(lexically_normalize(std::path::Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(lexically_normalize(std::path::Path::new("/a/b/..")), PathBuf::from("/a"));
        assert_eq!(lexically_normalize(std::path::Path::new("/a/./b")), PathBuf::from("/a/b"));
    }
}

mod navigation_tests {
    use super::*;

    // 5 entries, 2 columns -> rows = 3: col0 = [0,1,2], col1 = [3,4]
    // (col1's row 2 doesn't exist — 5 doesn't divide evenly by 2).

    #[test]
    fn move_down_flows_into_next_column_at_column_bottom() {
        let mut panel = panel_with(5, 2);
        panel.move_down();
        panel.move_down();
        assert_eq!(panel.selected, 2, "still col0, its last row");
        panel.move_down();
        assert_eq!(panel.selected, 3, "flowed into col1, row0");
    }

    #[test]
    fn move_down_clamped_at_last_entry() {
        let mut panel = panel_with(5, 2);
        panel.selected = 4;
        panel.move_down();
        assert_eq!(panel.selected, 4);
    }

    #[test]
    fn move_up_flows_into_previous_column_at_column_top() {
        let mut panel = panel_with(5, 2);
        panel.selected = 3; // col1, row0
        panel.move_up();
        assert_eq!(panel.selected, 2, "flowed into col0, its last row");
    }

    #[test]
    fn move_up_clamped_at_first_entry() {
        let mut panel = panel_with(5, 2);
        panel.move_up();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn move_right_lands_in_next_column_same_row() {
        let mut panel = panel_with(5, 2);
        panel.selected = 1; // col0, row1
        panel.move_right();
        assert_eq!(panel.selected, 4); // col1, row1
    }

    #[test]
    fn move_right_clamped_when_last_column_is_shorter() {
        let mut panel = panel_with(5, 2);
        panel.selected = 2; // col0, row2 -- col1 has no row2
        panel.move_right();
        assert_eq!(panel.selected, 2);
    }

    #[test]
    fn move_left_clamped_at_first_column() {
        let mut panel = panel_with(5, 2);
        panel.move_left();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn navigation_on_empty_panel_is_noop() {
        let mut panel = panel_with(0, 2);
        panel.move_up();
        panel.move_down();
        panel.move_left();
        panel.move_right();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn set_columns_reclamps_selected_after_shrinking() {
        let mut panel = panel_with(5, 2);
        panel.selected = 4;
        panel.entries.truncate(2);
        panel.set_columns(1);
        assert_eq!(panel.selected, 1);
    }
}

/// Regression tests for the real report: a directory with more
/// entries than fit the panel's own height had no scrolling
/// whatsoever -- `move_down` past the bottom of the visible window
/// just silently did nothing further, the same shape `panel_with`'s
/// own fixed row count already exercises for column-major math, now
/// windowed through `visible_rows`/`scroll_offset`.
mod scrolling_tests {
    use super::*;

    /// Regression test for the real report: 101 entries (a synthetic
    /// `..` plus 100 files), 2 columns, a panel only 39 rows tall --
    /// column 1 (right-hand) started at entry 51 instead of 39, since
    /// an earlier version of `column_height` used `rows()` (the
    /// whole list's own even-split row count, 51 here) for column
    /// boundaries even while scrolled, rather than the actually
    /// visible page's own row count.
    #[test]
    fn column_two_starts_right_after_column_one_leaves_off_when_scrolling() {
        let panel = panel_with_viewport(101, 2, 39);
        assert_eq!(panel.column_height(), 39, "sanity: rows() = 51, capped at visible_rows = 39");

        // Column 0 renders entries[0..39]; column 1 should render
        // entries[39..78] immediately after it, not entries[51..90]
        // (rows()-based).
        let column_one_start = panel.scroll_offset() + panel.column_height();
        assert_eq!(column_one_start, 39);
    }

    /// 20 single-column entries, 5 visible rows -- `set_visible_rows`
    /// is what `ui::draw_panel`/`main.rs::run` calls each frame with
    /// the panel's own actual inner height; nothing scrolls until
    /// it's been told what that height is.
    fn panel_with_viewport(count: usize, columns: usize, visible_rows: usize) -> Panel {
        let mut panel = panel_with(count, columns);
        panel.set_visible_rows(visible_rows);
        panel
    }

    /// Regression test for the real report and follow-up: pressing
    /// `Right` at the last (right-hand) column of the currently
    /// visible page must page-turn the whole grid forward by one
    /// column's width, landing on the new page's own column 0, same
    /// row -- not nudge the view by a single row the way `Down`
    /// does. Requested directly after a minimal-shift version left
    /// the cursor at an arbitrary, non-aligned row instead of at the
    /// top of the freshly revealed column.
    #[test]
    fn moving_right_past_the_last_column_pages_forward_by_a_whole_column() {
        let mut panel = panel_with_viewport(102, 2, 28); // rows() = 51, column_height = min(28, 51) = 28
        panel.selected = 28; // col1, row 0 (col0 = [0,28), col1 = [28,56))
        panel.ensure_selected_visible();
        assert_eq!(panel.scroll_offset(), 0, "sanity");

        panel.move_right();

        assert_eq!(panel.selected, 56, "should land on the new page's own column 0, row 0");
        assert_eq!(panel.scroll_offset(), 28, "should have paged forward by a whole column (28), not by one row");
    }

    /// The mirror of the test above: `Left` only pages the grid
    /// backward once it's actually leaving the currently visible
    /// window -- moving from column 1 back to column 0 of the
    /// *same* window (already scrolled, from the `Right` press
    /// above) doesn't scroll at all, exactly like `Right` moving
    /// from column 0 to column 1 doesn't; only the *second* `Left`,
    /// which would leave column 0's own page, actually pages
    /// backward.
    #[test]
    fn moving_left_past_the_first_column_pages_backward_by_a_whole_column() {
        let mut panel = panel_with_viewport(102, 2, 28);
        panel.selected = 28; // col1, row 0
        panel.ensure_selected_visible();
        panel.move_right(); // col1, row 0 of the window that scrolling forward revealed
        assert_eq!(panel.selected, 56, "sanity");
        assert_eq!(panel.scroll_offset(), 28, "sanity");

        panel.move_left(); // col0, row 0 -- still inside the same window
        assert_eq!(panel.selected, 28);
        assert_eq!(panel.scroll_offset(), 28, "moving from column 1 to column 0 within the same window shouldn't scroll");

        panel.move_left(); // now leaving column 0's own window

        assert_eq!(panel.selected, 0);
        assert_eq!(panel.scroll_offset(), 0, "should have paged backward by a whole column");
    }

    /// A `Right`/`Left` press that stays *within* the currently
    /// visible page (doesn't cross into the next/previous one)
    /// shouldn't scroll at all -- pagination only kicks in once the
    /// jump genuinely leaves the visible grid.
    #[test]
    fn moving_right_within_the_same_page_does_not_scroll() {
        let mut panel = panel_with_viewport(102, 2, 28);
        panel.selected = 0; // col0, row 0

        panel.move_right();

        assert_eq!(panel.selected, 28, "col1, row 0 -- still within the first page");
        assert_eq!(panel.scroll_offset(), 0);
    }

    /// Regression test for the real report: from the last (right-hand)
    /// column, with less than a full column's worth of entries left
    /// beyond the current page, `Right` did nothing at all instead of
    /// reaching the trailing entries that were still unreached (here,
    /// `generate_test_files.bat`, sorted after all the numbered
    /// files) -- `column_height * columns` (`102`) overshooting
    /// `entries.len()` was silently treated the same as "this row has
    /// no counterpart at all," when it should instead land on the
    /// very last entry.
    #[test]
    fn moving_right_from_the_last_column_with_a_partial_remainder_jumps_to_the_very_last_entry() {
        let mut panel = panel_with_viewport(102, 2, 39);
        panel.selected = 78; // col1 (the last column), well short of its own last row
        panel.scroll_offset = 24; // matches the real report's own already-scrolled state

        panel.move_right();

        assert_eq!(panel.selected, 101, "should jump straight to the very last entry");
    }

    #[test]
    fn no_scrolling_while_everything_already_fits() {
        let mut panel = panel_with_viewport(5, 1, 10);
        for _ in 0..4 {
            panel.move_down();
        }
        assert_eq!(panel.scroll_offset(), 0, "5 rows fit in 10 -- nothing should ever need to scroll");
    }

    /// The exact behavior requested directly: reaching the last
    /// *visible* row and pressing `Down` once more scrolls the
    /// whole view by exactly one row, keeping the cursor on the new
    /// bottom-most visible row -- not a full-page jump.
    #[test]
    fn moving_down_past_the_visible_window_scrolls_by_exactly_one_row() {
        let mut panel = panel_with_viewport(20, 1, 5); // rows 0..4 visible
        for _ in 0..4 {
            panel.move_down();
        }
        assert_eq!(panel.selected, 4, "sanity: cursor on the last visible row");
        assert_eq!(panel.scroll_offset(), 0, "sanity: hasn't needed to scroll yet");

        panel.move_down();

        assert_eq!(panel.selected, 5);
        assert_eq!(panel.scroll_offset(), 1, "should have scrolled by exactly one row, not a full page");
    }

    #[test]
    fn moving_up_past_the_top_of_the_visible_window_scrolls_back_by_one_row() {
        let mut panel = panel_with_viewport(20, 1, 5);
        for _ in 0..6 {
            panel.move_down(); // selected = 6, scroll_offset = 2 (rows 2..7 visible)
        }
        assert_eq!(panel.scroll_offset(), 2, "sanity");

        // Rows 2..7 are all still inside the visible window -- moving
        // up to row 2 (the window's own top edge) shouldn't scroll yet.
        for _ in 0..4 {
            panel.move_up();
        }
        assert_eq!(panel.selected, 2);
        assert_eq!(panel.scroll_offset(), 2, "moving up while still inside the visible window shouldn't scroll");

        panel.move_up();

        assert_eq!(panel.selected, 1);
        assert_eq!(panel.scroll_offset(), 1, "should have scrolled back by exactly one row");
    }

    /// Multi-column layout: every column shares the same
    /// `scroll_offset` (the whole grid slides together, not one
    /// column independently of the other), so scrolling while in
    /// the right-hand column still tracks correctly.
    #[test]
    fn scroll_offset_is_shared_across_columns() {
        // rows() = 10 (20 entries / 2 columns), column_height =
        // min(5, 10) = 5 -- genuinely more than one page, so
        // scrolling is possible; page_size = 2 * 5 = 10.
        let mut panel = panel_with_viewport(20, 2, 5);
        panel.selected = 4; // col0, last visible row
        panel.ensure_selected_visible();
        assert_eq!(panel.scroll_offset(), 0);

        panel.move_right(); // same row, col1 -- entry 4 + column_height(5) = 9
        assert_eq!(panel.selected, 9);
        assert_eq!(panel.scroll_offset(), 0, "still inside the current page -- no scroll needed yet");

        panel.move_down(); // entry 10 -- past the current page (page covers 0..10)
        assert_eq!(panel.selected, 10);
        assert_eq!(panel.scroll_offset(), 1, "should scroll by one row even when the move happened in the second column");
    }

    #[test]
    fn shrinking_visible_rows_reclamps_the_scroll_offset() {
        let mut panel = panel_with_viewport(20, 1, 5);
        for _ in 0..10 {
            panel.move_down();
        }
        assert_eq!(panel.scroll_offset(), 6, "sanity: scrolled to keep row 10 visible in a 5-row window");

        // Terminal resized taller -- more rows fit now, previous
        // offset would show trailing blank space past the last
        // entry if left unclamped.
        panel.set_visible_rows(15);

        assert_eq!(panel.scroll_offset(), 5, "20 entries in a 15-row window -- offset should clamp to the max useful value");
    }

    #[test]
    fn zero_visible_rows_disables_scroll_adjustment() {
        let mut panel = panel_with(20, 1); // visible_rows still 0 -- no set_visible_rows call
        for _ in 0..15 {
            panel.move_down();
        }
        assert_eq!(panel.scroll_offset(), 0, "shouldn't have adjusted anything before the renderer ever reports a real height");
    }
}

mod marks_tests {
    use super::*;

    /// A single-column panel with a real `..` entry first, followed by
    /// `count` dummy files -- for the `..`-exclusion tests below, since
    /// `panel_with` (used everywhere else in this file) only ever
    /// builds plain numbered files, no synthetic parent entry.
    fn panel_with_dotdot_and(count: usize) -> Panel {
        let mut entries = vec![Entry {
            name: "..".to_string(),
            is_dir: true,
            size: 0,
            modified: None,
        }];
        entries.extend((0..count).map(|i| Entry {
            name: i.to_string(),
            is_dir: false,
            size: 0,
            modified: None,
        }));
        Panel {
            path: PathBuf::new(),
            entries,
            selected: 0,
            columns: 1,
            scroll_offset: 0,
            visible_rows: 0,
            marked: HashSet::new(),
        }
    }

    #[test]
    fn toggle_mark_move_down_marks_the_current_row_then_moves() {
        let mut panel = panel_with(5, 1);
        panel.toggle_mark_move_down();
        assert!(panel.is_marked(0));
        assert_eq!(panel.selected, 1);
        assert!(!panel.is_marked(1), "the row moved onto shouldn't get marked too -- only the row that was current");
    }

    #[test]
    fn toggling_the_same_row_twice_unmarks_it() {
        let mut panel = panel_with(5, 1);
        panel.toggle_mark_move_down();
        panel.move_up();
        panel.toggle_mark_move_down();
        assert!(!panel.is_marked(0));
    }

    #[test]
    fn toggle_mark_move_up_marks_the_current_row_then_moves_up() {
        let mut panel = panel_with(5, 1);
        panel.selected = 2;
        panel.toggle_mark_move_up();
        assert!(panel.is_marked(2));
        assert_eq!(panel.selected, 1);
    }

    #[test]
    fn select_all_marks_every_entry_except_dotdot() {
        let mut panel = panel_with_dotdot_and(3);
        panel.select_all();
        assert!(!panel.is_marked(0), "\"..\" should never be marked");
        assert!(panel.is_marked(1));
        assert!(panel.is_marked(2));
        assert!(panel.is_marked(3));
    }

    /// Regression coverage for the real request: `Ctrl+Right` should
    /// mark the *whole column* the paginated jump crosses, not just the
    /// two endpoints. 6 entries, 2 columns -- `rows()` = 3, so with no
    /// `visible_rows` set (`column_height` falls back to `rows()`) a
    /// single `move_right` from entry 0 jumps straight to entry 3.
    #[test]
    fn toggle_mark_move_right_marks_every_row_the_column_jump_crosses() {
        let mut panel = panel_with(6, 2);
        panel.toggle_mark_move_right();
        assert_eq!(panel.selected, 3, "sanity: jumped a whole column (3 rows)");
        for i in 0..=3 {
            assert!(panel.is_marked(i), "entry {i} sits between the start and end of the jump");
        }
        assert!(!panel.is_marked(4));
        assert!(!panel.is_marked(5));
    }

    /// Mirror of the test above, for `Ctrl+Left`.
    #[test]
    fn toggle_mark_move_left_marks_every_row_the_column_jump_crosses() {
        let mut panel = panel_with(6, 2);
        panel.selected = 3;
        panel.toggle_mark_move_left();
        assert_eq!(panel.selected, 0, "sanity: jumped a whole column back");
        for i in 0..=3 {
            assert!(panel.is_marked(i), "entry {i} sits between the start and end of the jump");
        }
    }

    /// Marks are keyed by entry name (see `Panel::marked`'s own doc
    /// comment) specifically so they survive an in-place `reload`, but
    /// they should still be dropped on an actual directory change --
    /// otherwise they'd land on whatever unrelated entries happen to
    /// share a name/index in the new directory.
    #[test]
    fn changing_directory_clears_marks() {
        let dir = unique_scratch_dir("panel-marks");
        fs::write(dir.join("a.txt"), b"data").expect("write scratch file");
        fs::create_dir_all(dir.join("sub")).expect("create scratch subdir");
        let mut panel = Panel::new(dir).expect("open scratch panel");
        panel.select_all();
        assert!(!panel.marked.is_empty(), "sanity");

        panel.change_dir("sub").unwrap();

        assert!(panel.marked.is_empty(), "marks shouldn't carry over into an unrelated directory");
    }
}
