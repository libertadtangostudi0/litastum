use std::io;
use std::path::PathBuf;

use edtui::syntect::highlighting::Theme as SynTheme;
use ratatui_image::picker::Picker;

use crate::command_line::{self, builtin_profiles, CommandHistoryMenu, ShellProfile};
use crate::editor::{Editor, EditorKeymapMenu, EditorKeymapMode, EditorMenu};
use crate::explorer::{
    AddUserMenuItemState, DriveMenu, FindFileState, ImagePreviewState, MarkdownLinkSearchState, MarkdownPreviewState, Panel, UserMenuCommandEdit, UserMenuPromptState,
    UserMenuState,
};
use crate::theming::{MainMenu, PopupStyle, PopupStyleMenu, Theme, ThemeMenu};


/// What the app is currently showing. Only one at a time — there's no
/// split-screen browse-while-editing yet.
pub enum Mode {
    /// The dual-pane browser.
    Browsing,
    /// A file open for editing (F4) -- or, when `App::markdown_edit_preview`
    /// is `Some`, `F3` on a `.md`/`.markdown` file: the *same* variant,
    /// drawn split (editor left, live preview right,
    /// `ui::draw`'s own `left_columns`/`right_columns` computation)
    /// instead of full-screen, rather than a second, near-duplicate
    /// `Mode` -- reusing this one keeps every bit of `editor_keymap.rs`'s
    /// Save/Close/discard-confirm/`return_from_editor` logic exactly as
    /// it already was, since none of that cares *why* an `Editor` is
    /// open, just that one is.
    Editing(Editor),
    /// Editing was interrupted by `Esc` with unsaved changes: the
    /// editor is shown behind a "discard changes?" prompt rather than
    /// silently closing. Holds the editor so `Editor` moves straight
    /// back into `Editing` on cancel, with no data loss either way.
    /// Same split-vs-full-screen distinction as `Editing` above, driven
    /// by the same `App::markdown_edit_preview`.
    ConfirmDiscard(Editor),
    /// The built-in editor's own **F9** menu (`editor::EditorMenu`,
    /// distinct from the browsing screen's own F9 -> `MainMenu` below) --
    /// currently just one item, `Keybindings`, leading to
    /// `EditorKeymapMenu` right below. Holds the `Editor` this was
    /// opened over, same `ConfirmDiscard(Editor)` shape directly above,
    /// so `Esc`/`Enter` hand it straight back into `Mode::Editing` or
    /// down into `EditorKeymapMenu` either way. Only ever reached from
    /// plain full-screen editing (`Mode::Editing` with `App::markdown_edit_preview`
    /// still `None`) -- `editor::handle_editor_key` doesn't open this
    /// menu at all while a linked Markdown preview session is active, so
    /// the split-view rendering path never needs to know about either
    /// this or `EditorKeymapMenu`.
    EditorMenu(Editor, EditorMenu),
    /// `EditorMenu`'s own `Keybindings` item -- picking a key-binding
    /// scheme (`editor::EditorKeymapMode`). Same `Editor`-holding shape
    /// as `EditorMenu` right above; its own `Esc` closes straight back
    /// to `Mode::Editing` rather than stepping back up to `EditorMenu`,
    /// matching `theming::PopupStyleMenu`'s own "leaf `Esc` closes all
    /// the way out" convention.
    EditorKeymapMenu(Editor, EditorKeymapMenu),
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
    /// browsing a (possibly nested) `LitastumMenu.toml`.
    UserMenu(UserMenuState),
    /// Collecting a selected user-menu item's own `!?Label?Default!`
    /// answers before running it.
    UserMenuPrompt(UserMenuPromptState),
    /// `F2` (or the startup check in `main.rs`) found a `FarMenu.ini` --
    /// asks whether to port it (the path is `FarMenu.ini` itself)
    /// before browsing, rather than converting or reading it silently.
    /// Reported even when `LitastumMenu.toml` already exists too (see
    /// `explorer::user_menu::state::resolve_menu`'s own doc comment).
    ConfirmPortFarMenu(PathBuf),
    /// `Ins` on the user menu -- the add-item form. Holds the menu
    /// being edited so `Esc`/a finished add hands it straight back to
    /// `Mode::UserMenu`, same shape as `ConfirmDiscard(Editor)` above.
    AddUserMenuItem(UserMenuState, AddUserMenuItemState),
    /// A one-line, dismiss-on-any-key notification -- currently only
    /// used to tell the user where `FarMenu.ini` ended up after
    /// declining to port it (`explorer::user_menu::input::
    /// handle_confirm_port_far_menu_key`), but deliberately generic
    /// (just a `String`) rather than named after that one caller, since
    /// this app has no status-bar message surface otherwise (see
    /// `ui/confirm.rs`'s own doc comment on that gap).
    Info(String),
    /// `F3` on a supported image file -- the right panel's own file
    /// listing is replaced with a live preview of it
    /// (`explorer::image_preview::open_preview`,
    /// `ui::image_preview::draw_image_preview`) instead of the usual
    /// browser layout on that side. `Left`/`Right` cycle through every
    /// other image file in the same directory
    /// (`explorer::image_preview::handle_image_preview_key`); `Esc`/`F3`
    /// again closes it back to `Mode::Browsing`.
    ImagePreview(ImagePreviewState),
    /// `l` while the embedded Markdown preview has focus
    /// (`App::markdown_edit_preview`, see its own doc comment) -- a
    /// filterable, keyboard-driven list of every link in the document
    /// (`explorer::markdown_preview::MarkdownLinkSearchState`),
    /// requested directly as a reliable alternative to `Ctrl`+click
    /// (whose own row-based hit-testing drifts once word-wrap is
    /// involved -- see `MarkdownPreviewState::link_at`'s own doc
    /// comment). Holds the `Editor` "parked" here (it can't stay in
    /// `Mode::Editing` at the same time `Mode` is this) so `Esc`/`Enter`
    /// can hand it straight back to `Mode::Editing(editor)` -- the
    /// `MarkdownPreviewState` itself stays put in
    /// `App::markdown_edit_preview` throughout, never moved into `Mode`
    /// at all, since it's shared between this and `Mode::Editing`.
    MarkdownLinkSearch(Editor, MarkdownLinkSearchState),
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
    /// Which key-binding scheme new editor sessions open with -- see
    /// `editor::EditorKeymapMode`. Same loading/persistence shape as
    /// `popup_style` right above (loaded once at startup in `main.rs`,
    /// swappable at runtime through the built-in editor's own F9 menu),
    /// and every `Editor::open` call site threads this through as its
    /// own `keymap_mode` argument, the same way `syntax_theme` above is
    /// threaded through as `custom_syntax_theme`.
    pub editor_keymap_mode: EditorKeymapMode,
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
    /// Same idea as `editor_return_to`, for `F4` pressed on a `Commands`
    /// item in the `F2` user menu instead of the Find file results
    /// popup -- the editor there is opened on a scratch file holding
    /// just that item's own command(s)
    /// (`explorer::user_menu::input::open_edit_selected_command`), not
    /// the item's real backing `LitastumMenu.toml`, so closing it needs
    /// to both restore `Mode::UserMenu` *and* feed the scratch file's
    /// final contents back into the item
    /// (`explorer::user_menu::state::finish_command_edit`) -- more than
    /// `editor_return_to`'s own plain "which mode to restore" job, hence
    /// its own field rather than folding both into one enum. Only one
    /// of the two is ever meaningfully in flight at once, since the
    /// editor can only have been opened from one place.
    pub user_menu_command_edit: Option<UserMenuCommandEdit>,
    /// `F3`'s image preview: which rendering protocol the real terminal
    /// actually supports -- queried once, in `main()`, via
    /// `Picker::from_query_stdio()` (Sixel/Kitty/iTerm2 if the terminal
    /// answers, half-blocks otherwise), before the main loop starts
    /// reading keyboard events -- `ratatui_image`'s own docs require
    /// this ordering, since the query writes/reads raw escape sequences
    /// on stdio that could otherwise collide with (or be swallowed by)
    /// `crossterm`'s own event reader. `App::new` itself just sets
    /// `Picker::halfblocks()` (no real query -- keeps every test that
    /// builds an `App` via `test_support::test_app` from touching real
    /// stdio, same isolation reasoning as `command_history`/
    /// `search_history` above), overwritten in `main()` right after
    /// construction, same pattern those two already use.
    pub image_picker: Picker,
    /// Whether `crossterm`'s `EnableMouseCapture` is currently active --
    /// set by `explorer::markdown_preview::open_preview` right after it
    /// actually succeeds, cleared by `handle_markdown_preview_key`'s own
    /// `Esc`/`F3` close path right after `DisableMouseCapture` succeeds.
    /// Exists specifically so `main.rs::restore_terminal` knows whether
    /// it's safe to send `DisableMouseCapture` at all when the app
    /// exits -- reported as a real crash on Windows
    /// (`Error: 0: Initial console modes not set`, `crossterm`'s own
    /// Windows console backend errors out disabling mouse capture that
    /// was never enabled in the first place, since it has no saved
    /// "initial mode" to restore) when `F10` was pressed in a session
    /// that never opened a Markdown preview at all, so mouse capture had
    /// never been turned on. `restore_terminal` only includes
    /// `DisableMouseCapture` in its own cleanup when this is `true`.
    pub mouse_capture_enabled: bool,
    /// `F3` on a `.md`/`.markdown` file: a live rendered preview shown
    /// alongside the built-in editor (`Mode::Editing`/`ConfirmDiscard`,
    /// drawn split by `ui::draw` whenever this is `Some` -- see
    /// `Mode::Editing`'s own doc comment), refreshed on every `Ctrl+S`
    /// (`editor_keymap::handle_editor_key`'s own `Save` arm) so editing
    /// the source and checking the rendered result stays a single
    /// side-by-side workflow rather than a separate preview-then-edit
    /// round trip. Requested directly ("одновременно просматривать
    /// .md, редактировать его в левой панели и при сохранении смотреть
    /// что в правой"). Lives here rather than inside `Mode::Editing`'s
    /// own tuple so every *other* `Mode::Editing`/`ConfirmDiscard` call
    /// site (plain `F4`, Find file's own edit-selected-result, the user
    /// menu's scratch-file command editor) doesn't need to thread a
    /// second, almost-always-`None` field through -- `None` there is
    /// just this field never having been set. `app.active` (`0` =
    /// editor, `1` = preview) decides which side keyboard input reaches
    /// while this is `Some`; `Tab` toggles it (`main.rs::handle_key_event`).
    /// Cleared by `editor_keymap::return_from_editor` the moment the
    /// editor actually closes for good.
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
