use std::io;
use std::path::PathBuf;

use edtui::syntect::highlighting::Theme as SynTheme;

use crate::editor::Editor;
use crate::menu::MainMenu;
use crate::panel::Panel;
use crate::shell::{self, ShellProfile};
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
    /// The F9 top menu (`menu.rs`) — currently `Settings` leading to
    /// `Color schemes` (below).
    MainMenu(MainMenu),
    /// F8 was pressed on a real entry: shown as a "delete this?" prompt
    /// over the browser rather than deleting immediately — see
    /// `PendingDelete`.
    ConfirmDelete(PendingDelete),
    /// F5/F6 was pressed on a real entry: shown as a "copy/move to?"
    /// prompt with an editable destination path, defaulting to the
    /// *other* panel's directory — see `PendingTransfer`.
    ConfirmTransfer(PendingTransfer),
    /// The color-scheme picker, reached via F9 → Settings → Color
    /// schemes.
    ThemeMenu(ThemeMenu),
    /// The Ctrl+P shell-profile picker popup, shown over the browser.
    ShellMenu(ShellMenu),
}


/// The entry F8 was pressed on, held while `Mode::ConfirmDelete` asks
/// for confirmation — captured up front (rather than re-reading
/// `panel.current()` at confirm time) so the prompt still names and
/// deletes the right entry even if the cursor moves or the panel
/// reloads for some other reason while the prompt is up.
pub struct PendingDelete {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}


/// Which of F5/F6 opened `Mode::ConfirmTransfer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferOp {
    Copy,
    Move,
}


/// The entry F5/F6 was pressed on, held while `Mode::ConfirmTransfer`
/// asks for (and lets the user edit) a destination — captured up front
/// for the same reason as `PendingDelete`: the prompt should still act
/// on the entry it was opened for even if something else changed the
/// active panel's cursor in the meantime (not currently possible while
/// the prompt is up, but cheap to make robust to regardless).
pub struct PendingTransfer {
    pub operation: TransferOp,
    pub source: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// Editable text — defaults to the *other* panel's directory
    /// joined with `name` (or, for `Shift+F6` rename, the entry's own
    /// directory), but can be freely edited before confirming. Full
    /// cursor movement (`text_field.rs`), not the command line's own
    /// append/backspace-only editing — see `text_field.rs`'s module
    /// doc for why this field gets a real cursor and the command line
    /// doesn't.
    pub destination: String,
    /// Character index (not byte offset) into `destination` — see
    /// `text_field.rs`.
    pub cursor: usize,
    /// `Shift+Left`/`Shift+Right` selection anchor, `None` when nothing
    /// is selected — see `text_field.rs`'s selection section.
    pub selection_anchor: Option<usize>,
}


/// State for the `Ctrl+P` "pick a shell" popup — which row is
/// highlighted while it's open. No file discovery step like
/// `ThemeMenu::open` needs (the profile list is fixed, from
/// `App::shell_profiles`), so this is just a cursor position.
pub struct ShellMenu {
    pub selected: usize,
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
    /// The always-live command line at the bottom of the browser (Far
    /// Manager-style) — see `command_line.rs` for the editing logic
    /// and `.claude/rules/litastum-stack.md` for the design.
    pub command_line: String,
    /// Shells the command line can run typed input through — see
    /// `shell.rs`. Never empty; `active_shell` indexes into it.
    pub shell_profiles: Vec<ShellProfile>,
    pub active_shell: usize,
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
            command_line: String::new(),
            shell_profiles: shell::builtin_profiles(),
            active_shell: 0,
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
