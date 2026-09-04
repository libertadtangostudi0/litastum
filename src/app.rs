use std::io;
use std::path::PathBuf;

use edtui::syntect::highlighting::Theme as SynTheme;

use crate::editor::Editor;
use crate::panel::Panel;
use crate::theme::Theme;
use crate::theme_menu::ThemeMenu;


/// What the app is currently showing. Only one at a time — there's no
/// split-screen browse-while-editing yet.
pub enum Mode {
    /// The dual-pane browser.
    Browsing,
    /// A file open for editing (F4).
    Editing(Editor),
    /// Editing was interrupted by `Esc` with unsaved changes: the
    /// editor is shown behind a "discard changes?" prompt rather than
    /// silently closing. Holds the editor so `Editor` moves straight
    /// back into `Editing` on cancel, with no data loss either way.
    ConfirmDiscard(Editor),
    /// The F9 color-scheme picker popup, shown over the browser.
    ThemeMenu(ThemeMenu),
}


/// Top-level application state: the two file panels, which one
/// currently has keyboard focus, and the current mode.
pub struct App {
    pub panels: [Panel; 2],
    pub active: usize,
    pub mode: Mode,
    pub should_quit: bool,
    /// The live interface theme — panels, borders, F-key bar, etc.
    /// Loaded once at startup (`config::load_active_theme`) and
    /// swappable at runtime through the F9 menu.
    pub theme: Theme,
    /// The editor's syntax-highlighting theme, independent of `theme`
    /// (see `.claude/rules/litastum-theming.md` for why the two are
    /// kept separate) — `None` means "use the built-in named theme",
    /// the zero-config default. Every `Editor` opened during the
    /// session is handed a clone of whatever this currently is.
    pub syntax_theme: Option<SynTheme>,
}


impl App {
    /// Builds the app with both panels rooted at `start_dir`.
    pub fn new(start_dir: PathBuf, theme: Theme, syntax_theme: Option<SynTheme>) -> io::Result<Self> {
        let left = Panel::new(start_dir.clone())?;
        let right = Panel::new(start_dir)?;
        Ok(Self {
            panels: [left, right],
            active: 0,
            mode: Mode::Browsing,
            should_quit: false,
            theme,
            syntax_theme,
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
