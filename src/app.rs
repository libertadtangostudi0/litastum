use std::io;
use std::path::PathBuf;

use crate::panel::Panel;


/// Top-level application state: the two file panels and which one
/// currently has keyboard focus.
pub struct App {
    pub panels: [Panel; 2],
    pub active: usize,
    pub should_quit: bool,
}


impl App {
    /// Builds the app with both panels rooted at `start_dir`.
    pub fn new(start_dir: PathBuf) -> io::Result<Self> {
        let left = Panel::new(start_dir.clone())?;
        let right = Panel::new(start_dir)?;
        Ok(Self {
            panels: [left, right],
            active: 0,
            should_quit: false,
        })
    }


    /// The panel that currently has keyboard focus.
    pub fn active_panel(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }


    /// Switches keyboard focus to the other panel.
    pub fn toggle_active(&mut self) {
        self.active = 1 - self.active;
    }
}
