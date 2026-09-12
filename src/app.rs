use std::io;
use std::path::PathBuf;

use edtui::syntect::highlighting::Theme as SynTheme;

use crate::command_line::{self, builtin_profiles, CommandHistoryMenu, ShellProfile};
use crate::editor::Editor;
use crate::explorer::{DriveMenu, FindFileState, Panel, UserMenuPromptState, UserMenuState};
use crate::theming::{MainMenu, PopupStyle, PopupStyleMenu, Theme, ThemeMenu};


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
    /// F9 → Options → UI -- which popup chrome style is active.
    PopupStyleMenu(PopupStyleMenu),
    /// F9 → Commands → Find file.
    FindFile(FindFileState),
    /// F9 → Commands → History.
    CommandHistory(CommandHistoryMenu),
    /// `Alt+F1`/`Alt+F2` — the per-panel "change drive" popup.
    ChangeDrive(DriveMenu),
    /// `F2` — Far Manager's own user menu (`explorer::user_menu`),
    /// browsing a (possibly nested) `LitastumMenu.ini`/`FarMenu.ini`.
    UserMenu(UserMenuState),
    /// Collecting a selected user-menu item's own `!?Label?Default!`
    /// answers before running it.
    UserMenuPrompt(UserMenuPromptState),
}


/// One entry F8 is deleting, held (along with the rest of
/// `PendingDelete::entries`) while `Mode::ConfirmDelete` asks for
/// confirmation — captured up front (rather than re-reading the panel
/// at confirm time) so the prompt still names and deletes exactly the
/// entries it was opened for even if the cursor or the panel's marks
/// change while it's up.
pub struct DeleteEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// `Entry::size` at the time F8 was pressed -- shown in the delete
    /// popup (`ui/confirm.rs`) for a single entry, the first real use
    /// of that field (see its own doc comment in `explorer/entry.rs`).
    /// Meaningless for a directory (a filesystem's own reported size
    /// for a directory entry is some small OS-internal number, not its
    /// recursive contents' total), so the popup only displays it for a
    /// file.
    pub size: u64,
}


/// F8 was pressed: shown as a "delete this?" prompt over the browser
/// rather than deleting immediately.
pub struct PendingDelete {
    /// Every entry being deleted. Almost always one entry -- F8 pressed
    /// with nothing marked in the active panel, the entry under the
    /// cursor -- but every entry currently marked there when at least
    /// one is (`explorer::command::request_delete`), same "marked set
    /// wins over the cursor" rule `PendingTransfer::sources` uses for
    /// F5/F6. Never empty -- `request_delete` doesn't open this prompt
    /// at all if there'd be nothing in it.
    pub entries: Vec<DeleteEntry>,
}


/// Which of F5/F6 opened `Mode::ConfirmTransfer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferOp {
    Copy,
    Move,
}


/// One entry F5/F6/`Shift+F6` is transferring, held (along with the
/// rest of `PendingTransfer::sources`) while `Mode::ConfirmTransfer`
/// asks for a destination — captured up front for the same reason as
/// `PendingDelete`: the prompt should still act on exactly the entries
/// it was opened for even if something else changed the active panel's
/// cursor or marks in the meantime (not currently possible while the
/// prompt is up, but cheap to make robust to regardless).
pub struct TransferSource {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}


/// F5/F6/`Shift+F6` was pressed: shown as a "copy/move to?" prompt with
/// an editable destination, defaulting to the *other* panel's
/// directory. See `PendingTransfer::sources`'s own doc comment for the
/// single-entry-vs-marked-set distinction.
pub struct PendingTransfer {
    pub operation: TransferOp,
    /// Every entry being transferred. Almost always one entry --
    /// F5/F6/`Shift+F6` pressed with nothing marked in the active
    /// panel, the entry under the cursor -- but every entry currently
    /// marked there when at least one is (`explorer::command`'s own
    /// `transfer_sources`), Far Manager-style: once anything's marked,
    /// F5/F6 acts on the marked set instead of just the cursor,
    /// regardless of where the cursor itself happens to sit. Never
    /// empty -- `request_transfer` doesn't open this prompt at all if
    /// there'd be nothing in it.
    ///
    /// `Shift+F6` (rename) never reads the panel's marks at all and
    /// always builds exactly one -- a free-text destination field has
    /// no sensible way to rename several entries at once, so renaming
    /// stays scoped to the cursor entry regardless of what else is
    /// marked.
    pub sources: Vec<TransferSource>,
    /// Editable text — a single entry defaults to the *other* panel's
    /// directory joined with its name (or, for `Shift+F6` rename, the
    /// entry's own directory), the same full target *path* it's always
    /// been; several entries (`sources.len() > 1`) default to just the
    /// other panel's directory instead, a target *directory* each
    /// source's own name gets joined onto individually at transfer
    /// time (`confirm::run_confirmed_transfer`) -- there's no single
    /// path that could rename several entries at once, so a multi-entry
    /// transfer doesn't offer that the way a single-entry one does.
    /// Can be freely edited before confirming either way. Full cursor
    /// movement (`text_field.rs`), not the command line's own
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
    /// Which chrome flavor popups render with -- see `theming::PopupStyle`.
    /// Loaded once at startup (`main.rs`, same pattern as `active_shell`
    /// below, not `theme`/`syntax_theme` above -- reading it from
    /// `config.json` needs real disk I/O, which would otherwise leak
    /// into every test built through `test_support::test_app`) and
    /// swappable at runtime through F9 → Options → UI.
    pub popup_style: PopupStyle,
    /// The always-live command line at the bottom of the browser (Far
    /// Manager-style) — see `command_line.rs` for the editing logic
    /// and `.claude/rules/litastum-stack.md` for the design.
    pub command_line: String,
    /// Cursor position within `command_line`, as a character index (not
    /// byte offset — see `text_field.rs`'s own module doc). Always
    /// `command_line.chars().count()` (the end) except while a
    /// selection is active or has just been resolved — every other
    /// mutation site (running the command, `Esc`, Tab-completion,
    /// accepting a history suggestion/entry) resets it back to the end,
    /// since plain typing has nowhere else sensible to land. Bare
    /// `Left`/`Right` still can't move this (reserved for panel
    /// navigation, per `.claude/rules/litastum-command-line.md`) —
    /// only `Shift+Left`/`Right` and `Ctrl+Shift+Left`/`Right`
    /// (`command_line/browsing.rs`) touch it, reusing `text_field.rs`'s
    /// selection functions built for the Copy/Move destination field.
    pub command_line_cursor: usize,
    /// `Shift`/`Ctrl+Shift`+`Left`/`Right`'s live selection anchor in
    /// `command_line` — `None` means no selection. See
    /// `command_line_cursor`'s own doc for why the command line needed
    /// a real cursor position at all (it didn't, before this).
    pub command_line_selection_anchor: Option<usize>,
    /// A live `Tab`-cycling session over `command_line`'s current word,
    /// if one's in progress — `None` whenever nothing's being cycled.
    /// See `command_line::CompletionCycle`.
    pub command_line_completion: Option<command_line::CompletionCycle>,
    /// Highlighted row in the auto-popping history-suggestion list
    /// (`command_line::suggest_history`, `ui::draw_history_suggestions`)
    /// — reset to `0` on every edit to `command_line`, same shape as
    /// `CommandHistoryMenu::selected` but living directly on `App`
    /// since this list isn't a separate `Mode`, just an overlay shown
    /// while `Mode::Browsing`'s own command line has matches.
    pub command_line_suggestion_selected: usize,
    /// Set by `Tab`-accepting a history suggestion, cleared by any
    /// further edit to `command_line` (`insert_char`/`backspace`/`Esc`)
    /// — suppresses `ui::draw_history_suggestions` from immediately
    /// popping right back up, since the just-accepted command line is
    /// itself always a substring match of the entry it came from.
    /// Reported as a real annoyance: without this, accepting a
    /// suggestion did nothing visible because the same list reappeared
    /// unchanged on the very next frame.
    pub command_line_suggestion_dismissed: bool,
    /// Shells the command line can run typed input through — see
    /// `shell.rs`. Never empty; `active_shell` indexes into it.
    pub shell_profiles: Vec<ShellProfile>,
    pub active_shell: usize,
    /// Every command actually run from the command line, oldest first
    /// — see `command_line::record_history`/`MAX_HISTORY` and the F9 →
    /// Commands → History popup. Session-only, not persisted.
    pub command_history: Vec<String>,
    /// Every query typed into the built-in editor's `Ctrl+F` search box
    /// and closed with `Esc`, oldest first — see
    /// `editor::find_history::record_history` and the box's own ghost-
    /// text suggestion (`editor::find_history::suggest`). Persisted to
    /// its own file, separate from `command_history` above (see
    /// `editor::find_history::HISTORY_FILE`'s own doc comment).
    pub search_history: Vec<String>,
    /// Whether `Alt` is currently held down — drives which row
    /// `ui::draw_function_keys` shows, Far Manager-style.
    ///
    /// True hold/release tracking on Windows: `crossterm`'s Windows
    /// Console backend never delivers a standalone press/release event
    /// for a bare modifier key on its own (only modifier flags riding
    /// along with an actual keypress), so `main.rs::wait_for_event`
    /// polls the OS directly (`alt_key::is_physically_down`,
    /// `GetAsyncKeyState`) while otherwise idle and updates this the
    /// instant the physical key state changes — see `alt_key.rs`'s own
    /// doc for the full story.
    ///
    /// On other platforms there's no equivalent poll, so this falls
    /// back to the same approximation as before: set fresh in
    /// `main.rs::handle_event` from whether the *last processed key
    /// event* carried the `Alt` modifier. The alternate row appears the
    /// instant an `Alt+`-something is pressed (correct) but only
    /// reverts on the *next* key press without `Alt`, not the instant
    /// `Alt` itself is released.
    pub alt_held: bool,
    /// Where the built-in editor (F4) should hand control back once it
    /// closes, if that's somewhere other than the ordinary browser --
    /// `None` (the common case: F4 pressed from `Mode::Browsing`)
    /// returns to `Mode::Browsing`, same as always. Requested directly
    /// for the Find file results popup: pressing `F4` there opens the
    /// selected result for editing (`find_file/input.rs::edit_selected_result`),
    /// but closing the editor used to always drop back to plain
    /// browsing, losing the results list even though nothing about the
    /// search itself was done with. `Some(state)` restores
    /// `Mode::FindFile(state)` instead, taken (`Option::take`) exactly
    /// once by whichever path actually closes the editor for good
    /// (`editor_keymap::return_from_editor`) -- `Esc`-cancelling out of
    /// `Mode::ConfirmDiscard` back into the editor doesn't consume it,
    /// since the editor hasn't actually closed yet.
    pub editor_return_to: Option<FindFileState>,
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
            popup_style: PopupStyle::default(),
            command_line: String::new(),
            command_line_cursor: 0,
            command_line_selection_anchor: None,
            command_line_completion: None,
            command_line_suggestion_selected: 0,
            command_line_suggestion_dismissed: false,
            shell_profiles: builtin_profiles(),
            active_shell: 0,
            command_history: Vec::new(),
            search_history: Vec::new(),
            alt_held: false,
            editor_return_to: None,
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
