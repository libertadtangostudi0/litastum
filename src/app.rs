use std::io;
use std::path::PathBuf;

use edtui::syntect::highlighting::Theme as SynTheme;
use ratatui_image::picker::Picker;

use crate::choice_menu::ChoiceMenu;
use crate::command_line::{self, builtin_profiles, CommandHistoryMenu, ShellProfile};
use crate::compare::{CompareLineEndingMenu, CompareMenu, CompareState, LineEndingDisplay};
use crate::editor::{Editor, EditorKeymapMenu, EditorKeymapMode, EditorMenu};
use crate::explorer::{
    AddUserMenuItemState, DriveMenu, FindFileState, ImagePreviewState, MarkdownLinkSearchState, MarkdownPreviewState, Panel, UserMenuCommandEdit, UserMenuPromptState,
    UserMenuState,
};
use crate::text_field::TextField;
use crate::theming::{MainMenu, PopupStyle, PopupStyleMenu, Theme, ThemeMenu};


/// What the app is currently showing. Only one at a time — there's no
/// split-screen browse-while-editing yet.
pub enum Mode {
    /// The dual-pane browser.
    Browsing,
    /// A file open for editing (F4). With `App::markdown_edit_preview`
    /// set (`F3` on a `.md` file) it's drawn split, editor left and live
    /// preview right -- the same variant, so Save/Close/discard logic
    /// doesn't care why an `Editor` is open.
    Editing(Editor),
    /// `Esc` with unsaved changes: a "discard changes?" prompt over the
    /// editor. Holds the `Editor` so cancel hands it straight back.
    ConfirmDiscard(Editor),
    /// The editor's own F9 menu (`editor::EditorMenu`), holding the
    /// `Editor` it was opened over. Not reachable while a linked Markdown
    /// preview is open, so the split-view path never draws it.
    EditorMenu(Editor, EditorMenu),
    /// `EditorMenu` -> `Keybindings`: picks `editor::EditorKeymapMode`.
    /// `Esc` closes all the way back to `Mode::Editing`.
    EditorKeymapMenu(Editor, EditorKeymapMenu),
    /// The browser's F9 menu (`theming::menu`): Commands / Options.
    MainMenu(MainMenu),
    /// F8 was pressed on a real entry: shown as a "delete this?" prompt
    /// over the browser rather than deleting immediately — see
    /// `PendingDelete`.
    ConfirmDelete(PendingDelete),
    /// F5/F6 was pressed on a real entry: shown as a "copy/move to?"
    /// prompt with an editable destination path, defaulting to the
    /// *other* panel's directory — see `PendingTransfer`.
    ConfirmTransfer(PendingTransfer),
    /// The color-scheme picker, F9 -> Options -> Color schemes.
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
    /// browsing a (possibly nested) `LitastumMenu.toml`.
    UserMenu(UserMenuState),
    /// Collecting a selected user-menu item's own `!?Label?Default!`
    /// answers before running it.
    UserMenuPrompt(UserMenuPromptState),
    /// A `FarMenu.ini` was found (by `F2` or the startup check): asks
    /// whether to port it rather than converting or reading it silently.
    ConfirmPortFarMenu(PathBuf),
    /// `Ins` on the user menu -- the add-item form. Holds the menu
    /// being edited so `Esc`/a finished add hands it straight back to
    /// `Mode::UserMenu`, same shape as `ConfirmDiscard(Editor)` above.
    AddUserMenuItem(UserMenuState, AddUserMenuItemState),
    /// A one-line, dismiss-on-any-key notification (e.g. where
    /// `FarMenu.ini` ended up after declining to port it). Generic on
    /// purpose -- there's no status-bar message surface otherwise.
    Info(String),
    /// `F3` on an image file: the right panel shows a preview.
    /// `Left`/`Right` cycle through the directory's images; `Esc`/`F3`
    /// close it.
    ImagePreview(ImagePreviewState),
    /// `l` in the embedded Markdown preview: a filterable list of the
    /// document's links -- a reliable alternative to `Ctrl`+click, whose
    /// row hit-testing drifts with word-wrap. Parks the `Editor` here;
    /// the preview state stays in `App::markdown_edit_preview`.
    MarkdownLinkSearch(Editor, MarkdownLinkSearchState),
    /// `Alt+F5`: side-by-side compare of two files, both panes editable
    /// and kept row-aligned live. Design: `TODO/file-compare.md`.
    CompareFiles(CompareState),
    /// Compare's own F9 menu (`compare::CompareMenu`).
    CompareMenu(CompareState, CompareMenu),
    /// Compare's F9 -> Line endings (`compare::LineEndingDisplay`).
    CompareLineEndingMenu(CompareState, CompareLineEndingMenu),
    /// `Esc` on Compare with unsaved changes in either pane; holds the
    /// state so cancel hands it straight back.
    CompareConfirmDiscard(CompareState),
}


/// One entry F8 is deleting, captured when the prompt opens so it acts
/// on exactly those entries even if the cursor or marks change.
pub struct DeleteEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// Shown in the delete popup for a single file (a directory's
    /// reported size isn't its contents' total, so it's not shown).
    pub size: u64,
}


/// F8 was pressed: shown as a "delete this?" prompt over the browser
/// rather than deleting immediately.
pub struct PendingDelete {
    /// The marked entries if any are marked, else the cursor entry
    /// (same rule as `PendingTransfer::sources`). Never empty.
    pub entries: Vec<DeleteEntry>,
}


/// Which of F5/F6 opened `Mode::ConfirmTransfer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferOp {
    Copy,
    Move,
}


/// One entry F5/F6/`Shift+F6` is transferring, captured up front for
/// the same reason as `DeleteEntry`.
pub struct TransferSource {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}


/// F5/F6/`Shift+F6` was pressed: a "copy/move to?" prompt with an
/// editable destination.
pub struct PendingTransfer {
    pub operation: TransferOp,
    /// The marked entries if any are marked, else the cursor entry (Far
    /// Manager-style). Never empty. `Shift+F6` (rename) always uses just
    /// the cursor entry -- one text field can't rename several.
    pub sources: Vec<TransferSource>,
    /// A single entry defaults to a full target *path* (the other
    /// panel's directory + its name; its own directory for rename).
    /// Several entries default to a target *directory*, each name joined
    /// on at transfer time (`confirm::run_confirmed_transfer`). Edited
    /// with full cursor movement.
    pub destination: TextField,
}


/// The `Ctrl+P` shell picker, over indices into `App::shell_profiles`
/// (a runtime list, so indices rather than the profiles themselves).
pub type ShellMenu = ChoiceMenu<usize>;


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
    /// The editor's syntax theme, independent of `theme`
    /// (`.claude/rules/litastum-theming.md`); `None` = the built-in named
    /// theme. Each opened `Editor` gets a clone.
    pub syntax_theme: Option<SynTheme>,
    /// Which chrome flavor popups render with (`theming::PopupStyle`).
    /// Loaded in `main.rs`, not `App::new` -- reading `config.json` would
    /// otherwise leak disk I/O into every test built via `test_app`.
    pub popup_style: PopupStyle,
    /// Which key-binding scheme new editor sessions open with
    /// (`editor::EditorKeymapMode`). Loaded like `popup_style`; passed to
    /// every `Editor::open`.
    pub editor_keymap_mode: EditorKeymapMode,
    /// Whether Compare shows a per-line `CRLF`/`LF` marker. Loaded like
    /// `popup_style`.
    pub compare_line_ending_display: LineEndingDisplay,
    /// The always-live command line at the bottom of the browser (Far
    /// Manager-style) -- see `command_line::browsing` for the editing
    /// logic and `.claude/rules/litastum-command-line.md` for the design.
    /// The cursor stays at the end except while a selection is active:
    /// bare `Left`/`Right` belong to panel navigation, so only
    /// `Shift`/`Ctrl`+arrows move it.
    pub command_line: TextField,
    /// A live `Tab`-cycling session over `command_line`'s current word,
    /// if one's in progress — `None` whenever nothing's being cycled.
    /// See `command_line::CompletionCycle`.
    pub command_line_completion: Option<command_line::CompletionCycle>,
    /// Highlighted row in the history-suggestion overlay
    /// (`command_line::suggest_history`); reset on every edit. Lives on
    /// `App` because the overlay isn't a `Mode`.
    pub command_line_suggestion_selected: usize,
    /// Set by `Tab`-accepting a suggestion, cleared by the next edit --
    /// the accepted line always matches its own entry, so the list would
    /// otherwise pop straight back up.
    pub command_line_suggestion_dismissed: bool,
    /// Shells the command line can run typed input through — see
    /// `shell.rs`. Never empty; `active_shell` indexes into it.
    pub shell_profiles: Vec<ShellProfile>,
    pub active_shell: usize,
    /// Commands run from the command line, oldest first
    /// (`command_line::record_history`, capped by
    /// `Limits::max_command_history`); F9 -> Commands -> History.
    pub command_history: Vec<String>,
    /// Editor `Ctrl+F` queries, oldest first (`editor::find_history`),
    /// persisted to their own file.
    pub search_history: Vec<String>,
    /// Find file (`Alt+F7`) "File name to find" queries, oldest first
    /// (`explorer::find_file_history`). Name masks and content strings
    /// keep separate histories, each in its own file.
    pub find_file_name_history: Vec<String>,
    /// Find file "Text to find" queries -- see `find_file_name_history`.
    pub find_file_content_history: Vec<String>,
    /// Whether `Alt` is held -- selects the F-key bar's row, Far-style.
    /// On Windows a bare modifier produces no event, so it's polled
    /// (`windows_terminal::alt_key`); elsewhere it's read from the last
    /// key event's modifiers and only clears on the next key.
    pub alt_held: bool,
    /// Windows Terminal paste state (`windows_terminal::PasteFlood`).
    pub paste_flood: crate::windows_terminal::PasteFlood,
    /// `Some` when the editor was opened from Find file results: closing
    /// it for good (`editor_keymap::return_from_editor`) restores
    /// `Mode::FindFile` instead of plain browsing. Cancelling a discard
    /// prompt doesn't consume it.
    pub editor_return_to: Option<FindFileState>,
    /// Like `editor_return_to`, for `F4` on an `F2` menu item's commands:
    /// the editor works on a scratch file, and closing it both restores
    /// `Mode::UserMenu` and writes the result back into the item
    /// (`explorer::user_menu::state::finish_command_edit`).
    pub user_menu_command_edit: Option<UserMenuCommandEdit>,
    /// Image rendering protocol the terminal supports (Sixel/Kitty/iTerm2,
    /// else half-blocks). Queried in `main()` before the event loop reads
    /// input -- the query uses raw escape sequences on stdio. `App::new`
    /// uses half-blocks so tests never touch real stdio.
    pub image_picker: Picker,
    /// Whether mouse capture is on -- exactly while the editor is open,
    /// synced by `event_loop::sync_mouse_capture` and set only after the
    /// terminal call succeeded. `restore_terminal` sends
    /// `DisableMouseCapture` only when this is `true` (disabling it
    /// without enabling first crashes on Windows).
    pub mouse_capture_enabled: bool,
    /// `F3` on a `.md` file: the live preview shown beside the editor
    /// (`Mode::Editing` drawn split), refreshed on `Ctrl+S`. Lives here,
    /// not in `Mode::Editing`, so other editor call sites don't carry an
    /// always-`None` field. `app.active` (0 = editor, 1 = preview, `Tab`
    /// toggles) picks where keys go. Cleared when the editor closes.
    pub markdown_edit_preview: Option<MarkdownPreviewState>,
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
            editor_keymap_mode: EditorKeymapMode::default(),
            compare_line_ending_display: LineEndingDisplay::default(),
            command_line: TextField::new(),
            command_line_completion: None,
            command_line_suggestion_selected: 0,
            command_line_suggestion_dismissed: false,
            shell_profiles: builtin_profiles(),
            active_shell: 0,
            command_history: Vec::new(),
            search_history: Vec::new(),
            find_file_name_history: Vec::new(),
            find_file_content_history: Vec::new(),
            alt_held: false,
            paste_flood: crate::windows_terminal::PasteFlood::default(),
            editor_return_to: None,
            user_menu_command_edit: None,
            image_picker: Picker::halfblocks(),
            mouse_capture_enabled: false,
            markdown_edit_preview: None,
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


    /// Swaps the two panels' full contents (path, entries, cursor,
    /// scroll, marks) in place -- `self.active` is left untouched, so
    /// keyboard focus stays on the same screen *side*, only what's
    /// displayed there changes. Distinct from `toggle_active`, which
    /// moves focus without touching either panel's contents at all.
    /// Matches real Far Manager's own Ctrl+U.
    pub fn swap_panels(&mut self) {
        self.panels.swap(0, 1);
    }
}


#[cfg(test)]
mod tests {
    use crate::test_support::test_app;

    #[test]
    fn swap_panels_exchanges_contents_but_keeps_focus_on_the_same_side() {
        let dir = crate::test_support::unique_scratch_dir("app_swap_panels");
        let mut app = test_app(dir.clone());
        app.panels[0].selected = 3;
        app.panels[1].path = dir.join("other");
        app.panels[1].selected = 7;
        app.active = 0;

        app.swap_panels();

        assert_eq!(app.panels[0].path, dir.join("other"));
        assert_eq!(app.panels[0].selected, 7);
        assert_eq!(app.panels[1].path, dir);
        assert_eq!(app.panels[1].selected, 3);
        assert_eq!(app.active, 0);
    }
}
