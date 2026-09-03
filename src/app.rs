use std::io;
use std::path::PathBuf;

use crate::editor::Editor;
use crate::panel::Panel;


/// What the app is currently showing: the dual-pane browser, or a file
/// open for editing (F4). Only one at a time — there's no split-screen
/// browse-while-editing yet.
pub enum Mode {
    Browsing,
    Editing(Editor),
}


/// Top-level application state: the two file panels, which one
/// currently has keyboard focus, and the current mode.
pub struct App {
    pub panels: [Panel; 2],
    pub active: usize,
    pub mode: Mode,
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
            mode: Mode::Browsing,
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
