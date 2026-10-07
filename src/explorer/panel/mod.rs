use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use super::entry::Entry;

mod marks;
mod natural_sort;

use natural_sort::natural_compare;

/// One of the two file-list panes: its directory, entries and cursor.
///
/// Entries are laid out column-major (fill column 1 top to bottom, then
/// column 2, ...) within the visible page starting at `scroll_offset`:
/// entry `i`'s column is `(i - scroll_offset) / column_height()`. The
/// renderer writes `columns` and `visible_rows` each frame. History: docs/history/panel.md.
#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub columns: usize,
    /// Where the visible page starts in `entries`. The whole grid scrolls
    /// together, as in Far; kept in range by `ensure_selected_visible*`.
    scroll_offset: usize,
    /// Rows per column that fit on screen, written by the renderer each
    /// frame. `0` = not known yet (before the first frame): scrolling
    /// is left alone rather than computed from a guess.
    visible_rows: usize,
    /// Names of the marked entries (Far-style multi-select, `marks.rs`).
    /// Keyed by name so marks survive an in-place `reload()`; cleared on
    /// a real directory change.
    marked: HashSet<String>,
    /// Where the panel was last drawn, written by the renderer each frame
    /// (empty while something else is drawn in its place): a click on its
    /// title edits the path (`command_line::handle_browsing_mouse`).
    pub screen_area: ratatui::layout::Rect,
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
            screen_area: ratatui::layout::Rect::default(),
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


    /// Directories before files; within each group, case-insensitive and
    /// natural (`2.txt` before `10.txt`), like Far's panel.
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


    /// Rows per column if the whole list fit on screen. Layout and
    /// navigation use `column_height` instead.
    fn rows(&self) -> usize {
        if self.columns == 0 {
            return 0;
        }
        self.entries.len().div_ceil(self.columns)
    }


    /// The column height layout and navigation use: `min(visible_rows,
    /// rows())`. A list that fits splits evenly across the columns; a
    /// longer one becomes pages of `columns * visible_rows`. Plain
    /// `rows()` until the renderer has reported `visible_rows`.
    /// History: docs/history/panel.md.
    pub fn column_height(&self) -> usize {
        let rows = self.rows();
        if self.visible_rows == 0 || rows == 0 {
            rows
        } else {
            self.visible_rows.min(rows)
        }
    }


    /// The row count last reported by the renderer -- read back by
    /// `ui::draw` while a full-screen mode hides the panels, so there's
    /// nothing fresh to report.
    pub fn visible_rows(&self) -> usize {
        self.visible_rows
    }


    /// Sets the display column count — recomputed by the renderer each
    /// frame from the panel's on-screen width — and re-clamps the
    /// cursor in case a resize shrank the entry it pointed at.
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.ensure_selected_visible();
    }


    /// Sets the rows per column that fit on screen (renderer, each
    /// frame), re-checking the scroll position.
    pub fn set_visible_rows(&mut self, visible_rows: usize) {
        self.visible_rows = visible_rows;
        self.ensure_selected_visible();
    }


    /// Scrolls by the minimum needed to bring the cursor back into the
    /// visible page -- one row at a time at the edge, as in Far. A no-op
    /// before `visible_rows` is known.
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


    /// Like `ensure_selected_visible`, but pages a whole column at a
    /// time -- for `move_left`/`move_right`, whose jump is a whole
    /// column, so the cursor lands at the new column's edge.
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


    /// Moves one column left, same row, clamped at the first column.
    /// Jumps by `column_height`, not `rows()` -- they differ once
    /// scrolling.
    pub fn move_left(&mut self) {
        let column_height = self.column_height();
        if column_height > 0 {
            self.selected = self.selected.saturating_sub(column_height);
        }
        self.ensure_selected_visible_paginated();
    }


    /// Moves one column right, same row. Past the end of the list: stays
    /// put if the row just doesn't exist in the next column, but from
    /// the last column jumps to the very last entry (a partial page
    /// would otherwise be unreachable). History: docs/history/panel.md.
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


    /// Changes to `target`, relative to the current path (an absolute
    /// one replaces it) and lexically normalized (`cd ..` leaves a clean
    /// parent path). Returns `false`, panel untouched, if it isn't a
    /// real directory -- a multi-line F2 item stops on that instead of
    /// running the rest in the wrong place.
    pub fn change_dir(&mut self, target: &str) -> io::Result<bool> {
        let new_path = lexically_normalize(&self.path.join(target));
        if !new_path.is_dir() {
            return Ok(false);
        }
        self.path = new_path;
        self.selected = 0;
        self.marked.clear();
        self.reload()?;
        Ok(true)
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
