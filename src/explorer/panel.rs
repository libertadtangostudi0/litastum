use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::PathBuf;

use super::entry::Entry;

/// One of the two file-list panes. Holds its current directory, the
/// entries within it, and which entry the cursor is on.
///
/// The entry list is displayed column-major (fill column 1 top to
/// bottom, then column 2, ...) rather than as a single flat list, so
/// `entries[i]`'s column is `i / column_height()` and its row is
/// `i % column_height()`, relative to the currently visible page (see
/// `scroll_offset` below) -- not to the whole list at once.
/// `columns` is written by the renderer each frame from the panel's
/// on-screen width — see `ui::draw`.
///
/// `column_height()` -- the row count column boundaries are actually
/// computed from -- is `min(visible_rows, rows())`: as long as
/// everything fits on screen, it's the whole list's own even split
/// (`rows()`); once the list is longer than one page, the panel's own
/// height (`visible_rows`) takes over and it starts paginating
/// (`scroll_offset`) instead. Reported directly, twice: first that a
/// directory with more entries than fit the panel's own height had no
/// scrolling at all (`ui/mod.rs`'s renderer used to hand every one of a
/// column's own entries straight to a plain `ratatui::widgets::List`
/// with no `ListState`, which doesn't auto-scroll on its own); then,
/// once scrolling existed, that column 2 was starting at the wrong
/// entry -- an earlier version of this used `visible_rows` directly as
/// the column height unconditionally, which is correct only once
/// there's genuinely more than one page, and wrong for a short list
/// that already fits (see `column_height`'s own doc comment for the
/// exact failure shape).
#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub columns: usize,
    /// How far into the flat `entries` array the currently visible page
    /// starts -- the same value applies to every column at once (Far
    /// Manager's own multi-column scrolling: the whole grid slides
    /// together, not one column at a time). Kept in
    /// `[0, entries.len().saturating_sub(columns * column_height())]`
    /// by `ensure_selected_visible`, called after every cursor move.
    scroll_offset: usize,
    /// How many rows of each column actually fit on screen — written by
    /// the renderer each frame from the panel's own inner height, same
    /// pattern as `columns` above (`ui::draw_panel`/`main.rs::run`).
    /// `0` means "not yet known" (before the first real frame) and
    /// disables scroll adjustment entirely rather than dividing by
    /// zero or scrolling based on a stale guess.
    visible_rows: usize,
}


impl Panel {
    /// Creates a panel rooted at `path` and immediately loads its contents.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let mut panel = Self {
            path,
            entries: Vec::new(),
            selected: 0,
            columns: 1,
            scroll_offset: 0,
            visible_rows: 0,
        };
        panel.reload()?;
        Ok(panel)
    }


    /// Re-reads the current directory from disk, replacing `entries`.
    /// A synthetic ".." entry is inserted first when the directory has
    /// a parent, so the cursor can always navigate upward.
    pub fn reload(&mut self) -> io::Result<()> {
        let mut entries = Vec::new();

        if self.path.parent().is_some() {
            entries.push(Entry {
                name: "..".to_string(),
                is_dir: true,
                size: 0,
                modified: None,
            });
        }

        let mut children: Vec<Entry> = fs::read_dir(&self.path)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| Self::entry_from_dir_entry(&entry).ok())
            .collect();

        children.sort_by(Self::compare_entries);
        entries.extend(children);

        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.ensure_selected_visible();
        Ok(())
    }


    fn entry_from_dir_entry(entry: &fs::DirEntry) -> io::Result<Entry> {
        let metadata = entry.metadata()?;
        Ok(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }


    /// Directories sort before files; within each group, names sort
    /// case-insensitively and *naturally* -- digit runs compare by
    /// numeric value, not character-by-character, so `2.txt` sorts
    /// before `10.txt` the way Far Manager's own panel does. Reported
    /// directly against a real 100-file directory: plain lexicographic
    /// `to_lowercase().cmp()` put `100.txt` right after `10.txt` and
    /// before `11.txt`, since `"1"` < `"1"` ties and then `"0"` < `"1"`
    /// decides it -- every string-length-sensitive `N.txt` sequence
    /// beyond 9 entries reads out of order.
    fn compare_entries(a: &Entry, b: &Entry) -> Ordering {
        match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => natural_compare(&a.name, &b.name),
        }
    }


    /// The entry currently under the cursor, if the panel is non-empty.
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }


    /// Number of rows the list would need if it all fit on screen at
    /// once, split evenly into `columns` columns. Zero when there are
    /// no entries or no columns have been assigned yet. This is *not*
    /// what column-major layout actually uses once scrolling is
    /// possible — see `column_height` below, which is what navigation
    /// and rendering both read instead.
    fn rows(&self) -> usize {
        if self.columns == 0 {
            return 0;
        }
        self.entries.len().div_ceil(self.columns)
    }


    /// The row height actually used for column-major layout, both for
    /// navigation (`move_left`/`move_right`'s own jump size) and
    /// rendering (`ui::draw_entry_grid`'s own per-column slice) --
    /// `min(visible_rows, rows())`, deliberately not `visible_rows` on
    /// its own.
    ///
    /// Reported directly against a real 101-entry directory: with a
    /// naive `visible_rows`-as-column-height, column 1 started at
    /// `1 * visible_rows` in the flat entry list -- correct once
    /// there's genuinely more than fits (see the scrolling case below),
    /// but wrong for a short list that already fits within the panel's
    /// own height: `visible_rows` there is larger than `rows()` (the
    /// list's own natural per-column count), so column 1 would start
    /// far past where the *list itself* actually needs it to, leaving a
    /// tall gap of nothing in column 0 and every later column empty.
    /// Capping at `rows()` keeps a short list split evenly instead.
    ///
    /// Once the list is genuinely too long for one page (`rows() >
    /// visible_rows`), `visible_rows` takes over as the real limiting
    /// factor -- this is what turns "one flat list" into "one page of
    /// `columns * column_height` entries at a time," which
    /// `ensure_selected_visible` then scrolls through. Falls back to
    /// `rows()` outright before the renderer has ever reported a real
    /// `visible_rows` (`0`, the "not yet known" sentinel) -- e.g. the
    /// very first frame, or any test that never calls
    /// `set_visible_rows`.
    pub fn column_height(&self) -> usize {
        let rows = self.rows();
        if self.visible_rows == 0 || rows == 0 {
            rows
        } else {
            self.visible_rows.min(rows)
        }
    }


    /// Sets the display column count — recomputed by the renderer each
    /// frame from the panel's on-screen width — and re-clamps the
    /// cursor in case a resize shrank the entry it pointed at.
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.ensure_selected_visible();
    }


    /// Sets how many rows of each column actually fit on screen —
    /// recomputed by the renderer each frame from the panel's own inner
    /// height (`ui::draw_panel`), the same pattern `set_columns` above
    /// already follows for width. Re-checks the scroll position in case
    /// a resize changed how much is visible.
    pub fn set_visible_rows(&mut self, visible_rows: usize) {
        self.visible_rows = visible_rows;
        self.ensure_selected_visible();
    }


    /// Nudges `scroll_offset` by the minimum amount needed to bring the
    /// selected entry back inside the currently visible *page* --
    /// `[scroll_offset, scroll_offset + columns * column_height)` in the
    /// flat `entries` array -- never re-centers or jumps further than
    /// that, so a single `move_down`/`move_up` at the edge of the
    /// visible window scrolls by exactly one row, matching Far
    /// Manager's own multi-column scrolling (the whole grid slides
    /// together one row at a time, not a full screen at once). A no-op
    /// before the first real frame has reported `visible_rows` (`0` —
    /// nothing to clamp against yet) or on an empty panel.
    fn ensure_selected_visible(&mut self) {
        if self.visible_rows == 0 {
            return;
        }
        let column_height = self.column_height();
        let page_size = self.columns * column_height;
        if page_size == 0 {
            self.scroll_offset = 0;
            return;
        }

        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + page_size {
            self.scroll_offset = self.selected + 1 - page_size;
        }

        self.scroll_offset = if self.entries.len() > page_size { self.scroll_offset.min(self.entries.len() - page_size) } else { 0 };
    }


    /// Same purpose as `ensure_selected_visible` above, but for
    /// `move_left`/`move_right` specifically: crossing the edge of the
    /// currently visible page advances the whole page by one column's
    /// width at a time (paginating) rather than the minimal single-row
    /// nudge `move_up`/`move_down` use. Requested directly, after the
    /// minimal-shift version was tried first and reported feeling
    /// wrong here: unlike a single-row `Up`/`Down` step, a `Right`/
    /// `Left` press already jumps by a whole `column_height` in the
    /// flat list, so nudging the view by only one row left the cursor
    /// sitting at an arbitrary row instead of at the top/bottom edge of
    /// the newly-revealed column, where a page-turn puts it.
    fn ensure_selected_visible_paginated(&mut self) {
        if self.visible_rows == 0 {
            return;
        }
        let column_height = self.column_height();
        let page_size = self.columns * column_height;
        if page_size == 0 {
            self.scroll_offset = 0;
            return;
        }

        while self.selected < self.scroll_offset {
            self.scroll_offset = self.scroll_offset.saturating_sub(column_height);
        }
        while self.selected >= self.scroll_offset + page_size {
            self.scroll_offset += column_height;
        }

        self.scroll_offset = if self.entries.len() > page_size { self.scroll_offset.min(self.entries.len() - page_size) } else { 0 };
    }


    /// How far into the flat `entries` array the currently visible page
    /// starts — read by the renderer (`ui::draw_entry_grid`) to pick
    /// which entries land in which column this frame.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }


    /// Moves the cursor up one row. At the top of a column this flows
    /// into the bottom of the previous column — `entries` is already
    /// stored in column-major order, so a plain linear step does this
    /// correctly on its own. Clamped at the very first entry.
    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.ensure_selected_visible();
    }


    /// Moves the cursor down one row. At the bottom of a column this
    /// flows into the top of the next column, for the same reason as
    /// `move_up`. Clamped at the very last entry.
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
        self.ensure_selected_visible();
    }


    /// Moves the cursor one column left, keeping the same row, clamped
    /// at the first column. Jumps by `column_height` (the *currently
    /// visible* page's own row count), not the whole list's `rows()` --
    /// the two only differ once scrolling is actually happening, and
    /// jumping by `rows()` there would land in the wrong column
    /// entirely (see `column_height`'s own doc comment).
    pub fn move_left(&mut self) {
        let column_height = self.column_height();
        if column_height > 0 {
            self.selected = self.selected.saturating_sub(column_height);
        }
        self.ensure_selected_visible_paginated();
    }


    /// Moves the cursor one column right, keeping the same row. See
    /// `move_left`'s own doc comment for why this jumps by
    /// `column_height`, not `rows()`.
    ///
    /// When a full column-width jump would run past the end of the
    /// list, this means one of two different things, told apart by
    /// whether the cursor is already in the *last* column: if it
    /// isn't, the same row just doesn't exist in the next column at
    /// all (an unevenly divided short list, e.g. 5 entries in 2
    /// columns) -- stays put, same as always. If it *is* already the
    /// last column, there's nothing left to page a whole column at a
    /// time, but the list doesn't necessarily end exactly at the last
    /// full page either -- reported directly against a real 102-entry
    /// directory: from the last visible column, `Right` did nothing at
    /// all, even though there was a partial page's worth of entries
    /// (and the trailing `generate_test_files.bat`) still unreached.
    /// Jumps straight to the very last entry in that case, rather than
    /// silently ignoring the press. `move_left` doesn't need the same
    /// special case -- `saturating_sub` already floors at entry `0` on
    /// its own for the mirror situation.
    pub fn move_right(&mut self) {
        let column_height = self.column_height();
        if column_height == 0 {
            return;
        }
        let next = self.selected + column_height;
        if next < self.entries.len() {
            self.selected = next;
        } else if !self.entries.is_empty() {
            let current_column = (self.selected - self.scroll_offset) / column_height;
            if current_column + 1 >= self.columns {
                self.selected = self.entries.len() - 1;
            }
        }
        self.ensure_selected_visible_paginated();
    }


    /// Enters the directory under the cursor (or its parent, for "..").
    /// Does nothing if the current entry is a file.
    pub fn enter_selected(&mut self) -> io::Result<()> {
        let Some(entry) = self.current() else {
            return Ok(());
        };
        if !entry.is_dir {
            return Ok(());
        }

        let new_path = if entry.name == ".." {
            match self.path.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return Ok(()),
            }
        } else {
            self.path.join(&entry.name)
        };

        self.path = new_path;
        self.selected = 0;
        self.reload()
    }


    /// Full path to the entry currently under the cursor, if any.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.current().map(|entry| self.path.join(&entry.name))
    }


    /// Changes to `target`, resolved relative to the current path (an
    /// absolute `target` replaces it outright — `Path::join`'s own
    /// behavior, no separate case needed) and lexically normalized
    /// (`..` segments collapsed, so `cd ..` leaves a clean parent path
    /// rather than `.../sub/..` — found by a test that (correctly)
    /// expected the clean form). Used by the command line's `cd`
    /// handling (`command_line::parse_cd_target`); a target that
    /// doesn't resolve to a real directory is silently ignored rather
    /// than erroring, matching a real shell's tolerance for a typo'd
    /// `cd` not crashing anything.
    pub fn change_dir(&mut self, target: &str) -> io::Result<()> {
        let new_path = lexically_normalize(&self.path.join(target));
        if !new_path.is_dir() {
            return Ok(());
        }
        self.path = new_path;
        self.selected = 0;
        self.reload()
    }
}


/// Case-insensitive comparison that treats a run of digits as one
/// number, not a run of individual characters -- `"2.txt"` sorts before
/// `"10.txt"`, matching Far Manager's own panel and every other real
/// file manager. Walks both strings in lockstep, comparing plain
/// characters one at a time and digit runs (via `compare_digit_runs`)
/// as a whole, so it never needs to buffer more than one run at a time
/// or handle mixed content specially -- a name like `"v2.1.3"` still
/// compares each numeric segment (`2`, `1`, `3`) independently, exactly
/// as expected.
fn natural_compare(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    loop {
        return match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(ac), Some(bc)) if ac.is_ascii_digit() && bc.is_ascii_digit() => {
                let a_digits = take_digits(&mut a_chars);
                let b_digits = take_digits(&mut b_chars);
                match compare_digit_runs(&a_digits, &b_digits) {
                    Ordering::Equal => continue,
                    other => other,
                }
            }
            (Some(ac), Some(bc)) => match ac.to_ascii_lowercase().cmp(&bc.to_ascii_lowercase()) {
                Ordering::Equal => {
                    a_chars.next();
                    b_chars.next();
                    continue;
                }
                other => other,
            },
        };
    }
}

/// Consumes and returns the run of ASCII digits at the front of `chars`
/// (already confirmed non-empty by the caller).
fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut digits = String::new();
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_digit() {
            break;
        }
        digits.push(c);
        chars.next();
    }
    digits
}

/// Numeric comparison of two digit runs, without parsing into an
/// integer (a name could in principle have an absurdly long digit run —
/// this stays correct rather than overflowing or silently truncating).
/// Leading zeros are trimmed first so the comparison reflects the
/// *value*, not the digit run's own literal length (`"007"` and `"7"`
/// compare equal here) -- once trimmed, a longer digit run is always a
/// larger number, and equal-length runs compare the same lexicographically
/// as they would numerically.
fn compare_digit_runs(a: &str, b: &str) -> Ordering {
    let a_trimmed = a.trim_start_matches('0');
    let b_trimmed = b.trim_start_matches('0');
    a_trimmed.len().cmp(&b_trimmed.len()).then_with(|| a_trimmed.cmp(b_trimmed))
}


/// Collapses `.`/`..` path components without touching the filesystem
/// (no symlink resolution, unlike `fs::canonicalize` — which also
/// prepends the `\\?\` extended-length prefix on Windows, an ugly,
/// separate annoyance not worth taking on just to normalize `..`).
fn lexically_normalize(path: &std::path::Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}


#[cfg(test)]
mod tests {
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
            let mut panel = panel_with_viewport(101, 2, 39);
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

    mod natural_compare_tests {
        use super::*;

        #[test]
        fn digit_runs_compare_numerically_not_lexicographically() {
            let mut names = vec!["10.txt", "2.txt", "1.txt", "100.txt", "11.txt"];
            names.sort_by(|a, b| natural_compare(a, b));
            assert_eq!(names, vec!["1.txt", "2.txt", "10.txt", "11.txt", "100.txt"]);
        }

        #[test]
        fn plain_text_still_compares_case_insensitively() {
            assert_eq!(natural_compare("Banana", "apple"), Ordering::Greater);
            assert_eq!(natural_compare("apple", "Apple"), Ordering::Equal);
        }

        #[test]
        fn leading_zeros_compare_by_value_not_digit_count() {
            assert_eq!(natural_compare("007", "7"), Ordering::Equal);
            assert_eq!(natural_compare("007", "8"), Ordering::Less);
        }

        #[test]
        fn multiple_numeric_segments_each_compare_independently() {
            let mut names = vec!["v2.10.0", "v2.2.0", "v10.1.0"];
            names.sort_by(|a, b| natural_compare(a, b));
            assert_eq!(names, vec!["v2.2.0", "v2.10.0", "v10.1.0"]);
        }

        #[test]
        fn shorter_prefix_of_a_longer_string_sorts_first() {
            assert_eq!(natural_compare("file", "file2"), Ordering::Less);
        }
    }
}
