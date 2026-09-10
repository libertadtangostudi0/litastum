use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use super::entry::Entry;

mod marks;
mod natural_sort;

use natural_sort::natural_compare;

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
    /// Names of the entries currently marked (Far Manager-style
    /// multi-select) in this panel -- `Ctrl+A` (`select_all`) and
    /// `Ctrl+Up`/`Down`/`Left`/`Right` (`marks.rs`), rendered via
    /// `ui::build_list_item`. Keyed by name rather than index into
    /// `entries` so a mark survives an in-place `reload()` (a file
    /// changing on disk, or the panel being refreshed after an
    /// operation elsewhere) as long as the entry is still listed --
    /// an index-based set would silently point at the wrong entry the
    /// moment sorting or the entry count shifted. Cleared explicitly on
    /// an actual directory change (`enter_selected`/`change_dir`)
    /// instead, where carrying marks over would be meaningless (they'd
    /// almost certainly land on unrelated entries in the new
    /// directory).
    marked: HashSet<String>,
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
            marked: HashSet::new(),
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
        self.marked.clear();
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
        self.marked.clear();
        self.reload()
    }
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
mod tests;
