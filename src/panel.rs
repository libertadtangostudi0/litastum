use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;


/// A single entry (file or directory) shown in a panel's file list.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}


/// One of the two file-list panes. Holds its current directory, the
/// entries within it, and which entry the cursor is on.
///
/// The entry list is displayed column-major (fill column 1 top to
/// bottom, then column 2, ...) rather than as a single flat list, so
/// `entries[i]`'s column is `i / rows()` and its row is `i % rows()`.
/// `columns` is written by the renderer each frame from the panel's
/// on-screen width — see `ui::draw`.
#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub columns: usize,
}


impl Panel {
    /// Creates a panel rooted at `path` and immediately loads its contents.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let mut panel = Self {
            path,
            entries: Vec::new(),
            selected: 0,
            columns: 1,
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
    /// case-insensitively.
    fn compare_entries(a: &Entry, b: &Entry) -> Ordering {
        match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    }


    /// The entry currently under the cursor, if the panel is non-empty.
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }


    /// Number of display rows for the current column count. Zero when
    /// there are no entries or no columns have been assigned yet.
    fn rows(&self) -> usize {
        if self.columns == 0 {
            return 0;
        }
        self.entries.len().div_ceil(self.columns)
    }


    /// Sets the display column count — recomputed by the renderer each
    /// frame from the panel's on-screen width — and re-clamps the
    /// cursor in case a resize shrank the entry it pointed at.
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }


    /// Moves the cursor up within its column, clamped at the column's
    /// top row.
    pub fn move_up(&mut self) {
        let rows = self.rows();
        if rows > 0 && self.selected % rows > 0 {
            self.selected -= 1;
        }
    }


    /// Moves the cursor down within its column, clamped at the
    /// column's bottom row (the last column may be shorter than
    /// `rows()` when the entry count doesn't divide evenly).
    pub fn move_down(&mut self) {
        let rows = self.rows();
        if rows > 0 && self.selected % rows + 1 < rows && self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }


    /// Moves the cursor one column left, keeping the same row, clamped
    /// at the first column.
    pub fn move_left(&mut self) {
        let rows = self.rows();
        if rows > 0 {
            self.selected = self.selected.saturating_sub(rows);
        }
    }


    /// Moves the cursor one column right, keeping the same row,
    /// clamped at the last column.
    pub fn move_right(&mut self) {
        let rows = self.rows();
        if rows == 0 {
            return;
        }
        let next = self.selected + rows;
        if next < self.entries.len() {
            self.selected = next;
        }
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
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A panel with `count` dummy file entries, laid out in `columns`
    /// columns, cursor starting at index 0.
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
        }
    }

    // 5 entries, 2 columns -> rows = 3: col0 = [0,1,2], col1 = [3,4]
    // (col1's row 2 doesn't exist — 5 doesn't divide evenly by 2).

    #[test]
    fn move_down_stops_at_column_bottom() {
        let mut panel = panel_with(5, 2);
        panel.move_down();
        panel.move_down();
        assert_eq!(panel.selected, 2);
        panel.move_down();
        assert_eq!(panel.selected, 2, "col0 has no 4th row");
    }

    #[test]
    fn move_up_clamped_at_column_top() {
        let mut panel = panel_with(5, 2);
        panel.selected = 3; // col1, row0
        panel.move_up();
        assert_eq!(panel.selected, 3);
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
