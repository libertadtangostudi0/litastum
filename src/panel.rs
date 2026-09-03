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
#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
}


impl Panel {
    /// Creates a panel rooted at `path` and immediately loads its contents.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let mut panel = Self {
            path,
            entries: Vec::new(),
            selected: 0,
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


    /// Moves the cursor up by one row, clamped to the first entry.
    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    /// Moves the cursor down by one row, clamped to the last entry.
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
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
