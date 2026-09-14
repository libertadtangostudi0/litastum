use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::parse::{self, MenuItem, MenuItemBody, Prompt};
use super::toml_format;
use crate::theming::config::config_dir;

/// litastum's own user-menu file name -- a native, structured format
/// (`toml_format.rs`), not Far Manager's own hand-rolled DSL
/// (`parse.rs`). Picked over sticking with the same DSL specifically
/// so this app can read-modify-write it programmatically later
/// (adding/removing an item from the UI) without a bespoke serializer.
const OWN_FILE_NAME: &str = "LitastumMenu.toml";
/// Real Far Manager's own per-directory user-menu file name.
const FAR_FILE_NAME: &str = "FarMenu.ini";

/// What `F2` (or the startup check in `main.rs`) finds in a directory --
/// drives `explorer::command::open_user_menu`'s own branching between
/// browsing, offering to port a `FarMenu.ini`, or creating a fresh
/// file.
pub enum MenuFile {
    /// `LitastumMenu.toml` exists (and no `FarMenu.ini` is sitting next
    /// to it -- see `resolve_menu`'s own doc comment for why that takes
    /// priority) -- a malformed file still resolves to an empty item
    /// list rather than an error (see `toml_format::parse_toml`'s own
    /// doc comment), so this variant covers both cases. The `PathBuf`
    /// is the *directory* it was found in -- the active panel's own
    /// directory for a local menu, or the common config directory for
    /// the fallback below -- passed straight through to
    /// `UserMenuState::from_items` so edits persist back to wherever
    /// this particular menu actually came from.
    Own(PathBuf, Vec<MenuItem>),
    /// A `FarMenu.ini` is here -- offer to port it
    /// (`Mode::ConfirmPortFarMenu`) rather than reading or converting
    /// it silently. Reported *even if* `LitastumMenu.toml` also
    /// exists already -- dropping a `FarMenu.ini` into an already-
    /// configured directory should still surface the choice (port and
    /// overwrite, backing up the old config first, or decline and just
    /// have `FarMenu.ini` backed up out of the way) rather than being
    /// silently ignored.
    FarMenuFound(PathBuf),
    /// Neither file exists anywhere `resolve_menu` looked (the active
    /// directory nor the common config directory).
    NotFound,
}

/// Looks for a user menu in `dir` -- either directly, or (if nothing is
/// there at all) in the common config directory, so a menu set up once
/// is available from any directory on any drive, not just the one it
/// was created in. Real per-directory menus still always win: the
/// common one is only consulted when `dir` itself has neither file,
/// matching real Far Manager's own local-then-common precedence for
/// its `menu.ini`. Reported directly, against a real Subversion working
/// copy far from wherever the menu had actually been set up: switching
/// to another directory/drive showed an empty menu -- `resolve_menu`
/// used to only ever look at `dir`, so any directory without its own
/// `LitastumMenu.toml` showed nothing no matter what.
///
/// `config_dir()` (`theming::config`, reused here rather than
/// duplicated -- it's what every other per-user file, `config.json`/
/// `themes/`, already resolves through) also doubles as this app's one
/// local-development escape hatch: set `LITASTUM_CONFIG_DIR` to point
/// it at the project checkout instead of the real
/// `%APPDATA%\litastum\`, so testing this fallback doesn't mean
/// creating files in the real per-user config directory by hand. See
/// `config_dir`'s own doc comment.
///
/// The actual per-directory lookup (`resolve_menu_in`) is pulled out
/// separately so `resolve_menu_with_fallback` -- and this function's
/// own tests -- can exercise the local/common precedence with two
/// plain scratch directories, without touching the real config
/// directory at all (same "injectable path, untested wrapper" split
/// `theming::config`'s own tests already use, for the same reason:
/// exercising the real path would mutate whatever `LitastumMenu.toml`
/// a developer running the test suite actually has sitting in it).
pub fn resolve_menu(dir: &Path) -> MenuFile {
    resolve_menu_with_fallback(dir, config_dir().as_deref())
}

/// The one common menu location `resolve_menu` falls back to -- exposed
/// so `explorer::command::open_user_menu` can create a fresh
/// `LitastumMenu.toml` *there* (not in whichever directory happened to
/// be active) when `F2` finds nothing anywhere, per its own doc
/// comment. `None` if the platform gives us no config directory at
/// all (see `config_dir`'s own doc comment) -- same "just don't create
/// anything" fallback `create_menu_file`'s own failure case already
/// has.
pub fn common_menu_dir() -> Option<PathBuf> {
    config_dir()
}

/// An *empty* local `LitastumMenu.toml` (parses to zero items -- either
/// genuinely blank, or just the commented-out-example template
/// `create_menu_file` writes) doesn't count as "found" for fallback
/// purposes either -- reported directly: a directory where `F2` had
/// been pressed once before this fallback existed (creating that
/// template and nothing else) permanently shadowed the common menu
/// from then on, even though there was nothing real in the local file
/// to prefer over it. A local `FarMenu.ini`, or a local
/// `LitastumMenu.toml` with at least one real item, still always wins
/// -- this only widens what counts as "nothing here yet."
fn resolve_menu_with_fallback(dir: &Path, common_dir: Option<&Path>) -> MenuFile {
    match resolve_menu_in(dir) {
        Some(MenuFile::Own(local_dir, items)) if items.is_empty() => {
            common_dir.and_then(resolve_menu_in).unwrap_or(MenuFile::Own(local_dir, items))
        }
        Some(result) => result,
        None => common_dir.and_then(resolve_menu_in).unwrap_or(MenuFile::NotFound),
    }
}

/// A `FarMenu.ini` takes priority over an already-existing
/// `LitastumMenu.toml` -- reported directly: dropping a `FarMenu.ini`
/// into a directory that already has a configured menu used to be
/// silently ignored (this function returned `Own` without even
/// checking for `FarMenu.ini`), which meant there was no way to
/// deliberately re-import one short of deleting `LitastumMenu.toml`
/// first. Never writes anything itself -- porting (`port_far_menu`) or
/// backing `FarMenu.ini` out of the way (`backup_far_menu_without_porting`)
/// only happens once the user actually answers the prompt this
/// produces (`Mode::ConfirmPortFarMenu`). `None` if `dir` has neither
/// file, letting `resolve_menu_with_fallback` try the next directory.
fn resolve_menu_in(dir: &Path) -> Option<MenuFile> {
    let far = dir.join(FAR_FILE_NAME);
    if far.is_file() {
        return Some(MenuFile::FarMenuFound(far));
    }

    let own = dir.join(OWN_FILE_NAME);
    if own.is_file() {
        return Some(match fs::read_to_string(&own) {
            Ok(content) => MenuFile::Own(dir.to_path_buf(), toml_format::parse_toml(&content)),
            Err(err) => {
                warn!(path = %own.display(), %err, "LitastumMenu.toml exists but could not be read");
                MenuFile::Own(dir.to_path_buf(), Vec::new())
            }
        });
    }

    None
}


/// Backup suffix appended to whichever file `port_far_menu`/
/// `backup_far_menu_without_porting` move out of the way -- a single
/// slot, not a timestamped one: this is a rare, explicitly-confirmed
/// action, and clobbering an *older* backup on a second port is an
/// acceptable trade-off for not reinventing unique-file-name logic
/// that already exists (differently shaped) in `find_file/export.rs`.
const BACKUP_SUFFIX: &str = ".bak";

/// Ports `far_path` (a real `FarMenu.ini`) into a `LitastumMenu.toml`
/// alongside it -- called only once the user has confirmed it
/// (`Mode::ConfirmPortFarMenu`). Returns the parsed items regardless of
/// whether either write below actually succeeded (best-effort
/// persistence, same "never block on a failed write" rule
/// `theming::config` already follows for theme/setup persistence).
///
/// Two backups happen here, both requested directly after "what if I
/// already have a menu and drop a new FarMenu.ini in" came up:
/// - An *existing* `LitastumMenu.toml` is renamed to
///   `LitastumMenu.toml.bak` before being overwritten -- porting
///   shouldn't silently discard a menu someone already built by hand
///   or through the UI.
/// - `FarMenu.ini` itself is renamed to `FarMenu.ini.bak` once ported --
///   unlike the very first version of this feature (which left it
///   completely untouched), it has to move out of the way now that
///   `resolve_menu` reports a `FarMenu.ini`'s mere presence every time
///   regardless of whether `LitastumMenu.toml` already exists; leaving
///   it in place would re-trigger this same prompt on every future
///   `F2`/startup check.
pub fn port_far_menu(far_path: &Path) -> Vec<MenuItem> {
    let dir = far_path.parent().unwrap_or_else(|| Path::new("."));
    let content = match read_text_file_any_encoding(far_path) {
        Ok(content) => content,
        Err(err) => {
            warn!(path = %far_path.display(), %err, "FarMenu.ini could not be read for porting");
            return Vec::new();
        }
    };

    let (items, toml) = toml_format::port_ini_to_toml(&content);
    let own = dir.join(OWN_FILE_NAME);

    if own.is_file() {
        let backup = dir.join(format!("{OWN_FILE_NAME}{BACKUP_SUFFIX}"));
        if let Err(err) = fs::rename(&own, &backup) {
            warn!(path = %backup.display(), %err, "failed to back up the existing LitastumMenu.toml before overwriting it");
        }
    }
    if let Err(err) = fs::write(&own, &toml) {
        warn!(path = %own.display(), %err, "failed to write LitastumMenu.toml after porting FarMenu.ini");
    }

    let far_backup = far_path.with_file_name(format!("{FAR_FILE_NAME}{BACKUP_SUFFIX}"));
    if let Err(err) = fs::rename(far_path, &far_backup) {
        warn!(path = %far_backup.display(), %err, "failed to move FarMenu.ini aside after porting it");
    }

    items
}


/// `N`/`Esc` on the "port FarMenu.ini?" prompt: moves `far_path` aside
/// to `FarMenu.ini.bak` without reading or converting it, purely so it
/// stops being detected (and re-prompted for) on every future `F2`/
/// startup check -- `resolve_menu` reports a `FarMenu.ini`'s mere
/// presence unconditionally, so declining still has to make it go away
/// somehow. Returns the backup path so the caller can tell the user
/// where it ended up; `None` if the rename itself failed (permissions,
/// ...), logged and left as a silent no-op otherwise -- the file simply
/// stays in place and gets offered again next time.
pub fn backup_far_menu_without_porting(far_path: &Path) -> Option<PathBuf> {
    let backup = far_path.with_file_name(format!("{FAR_FILE_NAME}{BACKUP_SUFFIX}"));
    match fs::rename(far_path, &backup) {
        Ok(()) => Some(backup),
        Err(err) => {
            warn!(path = %backup.display(), %err, "failed to back up FarMenu.ini");
            None
        }
    }
}


/// Reads `path` as text, decoding whichever of UTF-8, UTF-16LE, or
/// UTF-16BE it's actually encoded in -- reported directly: a real
/// `FarMenu.ini` (exported straight from an actual Far Manager
/// install) failed to port at all, silently producing zero items.
/// Confirmed by inspecting the raw bytes: it starts with `FF FE` (a
/// UTF-16LE byte-order mark) followed by every character null-padded
/// -- real Far Manager saves this file in UTF-16LE, not UTF-8, and a
/// plain `fs::read_to_string` (strict UTF-8) fails outright on it,
/// since two-byte-per-character text is essentially never valid UTF-8.
/// Detected by byte-order mark, the same convention every other text
/// tool uses to tell these apart, since nothing else in the file names
/// its own encoding. Falls back to plain UTF-8 (also stripping a UTF-8
/// BOM, `EF BB BF`, if present) when there's no UTF-16 BOM -- covers a
/// hand-written or already-UTF-8 `FarMenu.ini` too, not just Far
/// Manager's own default export.
fn read_text_file_any_encoding(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;

    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok(decode_utf16(rest, u16::from_le_bytes));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok(decode_utf16(rest, u16::from_be_bytes));
    }

    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

/// Pairs up `bytes` two at a time (via `from_units`, `u16::from_le_bytes`
/// or `u16::from_be_bytes`) into UTF-16 code units and decodes them --
/// lossy (`char::REPLACEMENT_CHARACTER` for anything malformed) rather
/// than failing outright, since a slightly-corrupt menu file should
/// still port whatever of it *does* decode rather than porting nothing
/// at all. A trailing odd byte (a malformed file missing its last low
/// or high byte) is simply dropped by `chunks_exact(2)`.
fn decode_utf16(bytes: &[u8], from_units: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| from_units([pair[0], pair[1]])).collect();
    String::from_utf16_lossy(&units)
}


/// The commented-out-example content `create_menu_file` writes --
/// pulled out to a module-level constant (rather than local to that
/// function) so `resolve_menu_with_fallback`'s own tests can write the
/// exact same "empty template" content a real freshly-created file
/// would have, without duplicating it out of sync.
const EMPTY_MENU_TEMPLATE: &str = "\
# LitastumMenu.toml -- F2 user menu. Uncomment and edit:
#
# [[item]]
# title = \"status\"
# hotkey = \"s\"
# commands = [\"git status -s\"]
#
# [[item]]
# title = \"submenu example\"
# [[item.submenu]]
# title = \"nested item\"
# commands = [\"echo hi\"]
";

/// Creates a fresh `LitastumMenu.toml` in `dir`, with a commented-out
/// example to get started -- `F2` calls this when `resolve_menu` finds
/// neither file at all, so there's actually something to open in the
/// built-in editor right away (`explorer::command::open_user_menu`)
/// instead of an empty popup with nothing in it to select. `None` if
/// either step fails (a read-only directory, permissions, ...) -- `F2`
/// just does nothing then, same as any other "couldn't act on this"
/// case in this codebase.
///
/// `create_dir_all`s `dir` first, unlike the very first version of
/// this function -- needed once `open_user_menu` started passing the
/// *common config* directory here instead of the always-already-real
/// active panel directory: the OS config directory (`%APPDATA%\litastum\`
/// or equivalent) may not exist yet at all on a machine where no
/// theme/setup has ever been saved, and a plain `fs::write` fails
/// outright when its parent directory is missing. A no-op for the
/// already-real active-directory case this function still also serves
/// (`create_menu_file_tests`, `UserMenuState`'s own persistence).
pub fn create_menu_file(dir: &Path) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let path = dir.join(OWN_FILE_NAME);
    fs::write(&path, EMPTY_MENU_TEMPLATE).ok()?;
    Some(path)
}


/// A read-only view of whichever level `UserMenuState` currently has
/// on screen -- its items, and which one the cursor is on. Borrowed
/// fresh from the single canonical tree (`UserMenuState::root`) on
/// every call rather than stored, so an edit at any depth
/// (`insert_item`/`delete_selected`) is immediately reflected without
/// a separate "sync the clone back up" step -- see `UserMenuState`'s
/// own doc comment for why that matters.
pub struct UserMenuLevelView<'a> {
    pub items: &'a [MenuItem],
    pub selected: usize,
}


/// `F2`: browsing a (possibly nested) user menu, and adding/removing
/// items in place. `root` is the *single* canonical tree (what actually
/// gets persisted); `stack` is the path of indices walked from `root`
/// to reach whichever level is currently shown, `selected` the cursor
/// within that level. An earlier version cloned each submenu's children
/// into its own owned level on `enter_submenu` -- fine for read-only
/// browsing, but wrong the moment editing was added: an edit made three
/// levels deep would only ever touch that level's own disposable clone,
/// invisible to `root` and lost the instant `back()` popped it. Walking
/// `root` through `stack` on every access instead means there is
/// nowhere else for the data to live, so an edit at any depth is
/// automatically visible everywhere (including after `persist`ing to
/// `LitastumMenu.toml`) with no separate sync step.
pub struct UserMenuState {
    root: Vec<MenuItem>,
    /// Indices into progressively deeper `Submenu` levels, parent to
    /// child -- `stack.last()`'s value is which item *of the current
    /// level's own parent* was entered to get here (kept so `back()`
    /// can restore the cursor to exactly that item, same as the old
    /// per-level `selected` used to).
    stack: Vec<usize>,
    selected: usize,
    /// Where `LitastumMenu.toml` lives -- `insert_item`/`delete_selected`
    /// write `root` back here after every change.
    dir: PathBuf,
}

impl UserMenuState {
    /// Builds the browsing state from an already-resolved item list
    /// (`MenuFile::Own`, or the result of `port_far_menu`) -- file
    /// resolution itself lives in `resolve_menu`/`port_far_menu` above,
    /// kept separate so `explorer::command::open_user_menu` can decide
    /// what to do (browse, offer to port, or create) before ever
    /// building one of these. `dir` is where edits get persisted back
    /// to (`LitastumMenu.toml`), independent of wherever `items`
    /// itself originally came from (a fresh read, or a just-completed
    /// port).
    pub fn from_items(dir: PathBuf, items: Vec<MenuItem>) -> Self {
        Self { root: items, stack: Vec::new(), selected: 0, dir }
    }

    /// Walks `root` down through `stack` to whichever level is
    /// currently shown.
    fn current_items(&self) -> &[MenuItem] {
        let mut items = &self.root;
        for &index in &self.stack {
            let MenuItemBody::Submenu(children) = &items[index].body else {
                unreachable!("stack only ever holds indices enter_submenu confirmed pointed at a Submenu");
            };
            items = children;
        }
        items
    }

    fn current_items_mut(&mut self) -> &mut Vec<MenuItem> {
        let mut items = &mut self.root;
        for &index in &self.stack {
            let MenuItemBody::Submenu(children) = &mut items[index].body else {
                unreachable!("see current_items's own comment")
            };
            items = children;
        }
        items
    }

    pub fn current_level(&self) -> UserMenuLevelView<'_> {
        UserMenuLevelView { items: self.current_items(), selected: self.selected }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.current_items().len() {
            self.selected += 1;
        }
    }

    pub fn selected_item(&self) -> Option<&MenuItem> {
        self.current_items().get(self.selected)
    }

    /// Moves the cursor to the first item at the *current* level whose
    /// own `hotkey` matches `c` (case-insensitively, matching real Far
    /// Manager's own convention -- its hotkeys aren't case-sensitive
    /// either). `true` if one was found and the cursor moved there --
    /// the caller (`input::handle_user_menu_key`) still has to actually
    /// run/descend into it itself, same as it would for a plain `Enter`
    /// press once the cursor is already sitting on the right item.
    /// `false` (a silent no-op, cursor unchanged) for an unmatched
    /// letter or an empty level -- deliberately narrow: real Far only
    /// ever jumps within the level currently on screen, never searches
    /// into a collapsed submenu.
    pub fn select_by_hotkey(&mut self, c: char) -> bool {
        let Some(index) = self.current_items().iter().position(|item| item.hotkey.is_some_and(|hotkey| hotkey.eq_ignore_ascii_case(&c))) else {
            return false;
        };
        self.selected = index;
        true
    }

    /// Descends into the highlighted item if it's a submenu -- `true`
    /// on success. A no-op (`false`) for a `Commands` item or an empty
    /// level; the caller is expected to try running it as commands
    /// instead in that case.
    pub fn enter_submenu(&mut self) -> bool {
        if !matches!(self.selected_item(), Some(MenuItem { body: MenuItemBody::Submenu(_), .. })) {
            return false;
        }
        self.stack.push(self.selected);
        self.selected = 0;
        true
    }

    /// Backs up one level. `true` if it moved up a level (the caller
    /// stays in the menu); `false` if already at the top level (the
    /// caller should close the menu entirely) -- same contract as
    /// `theming::MainMenu::back`.
    pub fn back(&mut self) -> bool {
        let Some(parent_selected) = self.stack.pop() else {
            return false;
        };
        self.selected = parent_selected;
        true
    }

    /// Inserts `item` right after the highlighted one at the current
    /// level (or at the very start of an empty level), selects it, and
    /// persists the whole tree to `LitastumMenu.toml`.
    pub fn insert_item(&mut self, item: MenuItem) {
        let insert_at = if self.current_items().is_empty() { 0 } else { self.selected + 1 };
        self.current_items_mut().insert(insert_at, item);
        self.selected = insert_at;
        self.persist();
    }

    /// Removes the highlighted item at the current level (a no-op on
    /// an empty level), clamps the cursor to what's left, and persists.
    pub fn delete_selected(&mut self) {
        let selected = self.selected;
        let items = self.current_items_mut();
        if items.is_empty() {
            return;
        }
        items.remove(selected);
        let remaining = self.current_items().len();
        if self.selected >= remaining {
            self.selected = remaining.saturating_sub(1);
        }
        self.persist();
    }

    /// `F4` on a `Commands` item: replaces its own command list with
    /// `commands` in place (title and hotkey untouched) and persists --
    /// a no-op if `commands` is empty, the selected item isn't a
    /// `Commands` leaf, or the level is empty (nothing selected).
    /// `finish_command_edit` is the actual `F4` entry point that calls
    /// this, after reading the edited commands back from the real
    /// built-in editor.
    pub fn replace_selected_commands(&mut self, commands: Vec<String>) {
        if commands.is_empty() {
            return;
        }
        let selected = self.selected;
        let Some(item) = self.current_items_mut().get_mut(selected) else {
            return;
        };
        if !matches!(item.body, MenuItemBody::Commands(_)) {
            return;
        }
        item.body = MenuItemBody::Commands(commands);
        self.persist();
    }

    /// Writes `root` back to `LitastumMenu.toml` in `dir` -- best-effort,
    /// same "never block on a failed write" rule as everywhere else
    /// file persistence happens in this app; a failure just means the
    /// in-memory edit (still visible for the rest of this session)
    /// didn't make it to disk.
    fn persist(&self) {
        let toml = toml_format::to_toml_string(&self.root);
        let path = self.dir.join(OWN_FILE_NAME);
        if let Err(err) = fs::write(&path, &toml) {
            warn!(path = %path.display(), %err, "failed to save LitastumMenu.toml after editing the user menu");
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddItemStage {
    Title,
    Command,
}

/// `Mode::AddUserMenuItem`: a small two-field form (`Ins` while
/// browsing the user menu) for adding a new item without leaving the
/// popup to hand-edit `LitastumMenu.toml`. Deliberately minimal -- no
/// hotkey field, no multi-command items, no authoring help for `!&`/
/// `!?Label?Default!` -- those are still easiest to add by hand-editing
/// the file afterward; this covers the common case (one title, one
/// command, or a bare submenu to build out by entering it and adding
/// more items the same way).
pub struct AddUserMenuItemState {
    stage: AddItemStage,
    pub title: String,
    pub title_cursor: usize,
    pub title_selection_anchor: Option<usize>,
    pub command: String,
    pub command_cursor: usize,
    pub command_selection_anchor: Option<usize>,
}

impl AddUserMenuItemState {
    pub fn new() -> Self {
        Self {
            stage: AddItemStage::Title,
            title: String::new(),
            title_cursor: 0,
            title_selection_anchor: None,
            command: String::new(),
            command_cursor: 0,
            command_selection_anchor: None,
        }
    }

    pub fn is_title_stage(&self) -> bool {
        self.stage == AddItemStage::Title
    }

    /// `Enter` on the title field -- advances to the command field if
    /// the title isn't blank (`true`), a no-op otherwise (`false`):
    /// there's nothing sensible to call a titleless menu item.
    pub fn advance_from_title(&mut self) -> bool {
        if self.title.trim().is_empty() {
            return false;
        }
        self.stage = AddItemStage::Command;
        true
    }

    /// `Enter` on the command field -- builds the finished item: a
    /// `Commands` leaf if `command` has anything in it, an empty
    /// `Submenu` otherwise (entering it right afterward and adding more
    /// items the same way is how a submenu actually gets built out).
    pub fn finish(&self) -> MenuItem {
        let title = self.title.trim().to_string();
        let command = self.command.trim();
        let body = if command.is_empty() { MenuItemBody::Submenu(Vec::new()) } else { MenuItemBody::Commands(vec![command.to_string()]) };
        MenuItem { hotkey: None, title, body }
    }
}

impl Default for AddUserMenuItemState {
    fn default() -> Self {
        Self::new()
    }
}


/// `F4` on a highlighted `Commands` item: editing just that item's own
/// command(s) in the real built-in editor, not a bespoke single-line UI
/// form and not the whole `LitastumMenu.toml` file. Reported directly,
/// twice: a first attempt opened the whole file
/// (`explorer::user_menu::input::edit_menu_file`, since removed), a
/// second opened a small in-popup text field
/// (`EditUserMenuItemState`, since removed) -- both missed the actual
/// ask ("хочется редактировать не весь конфиг, а только команду/ы
/// внутри элемента, но в редакторе" -- just this item's command(s), but
/// in the real editor, with its own undo/syntax highlighting/multi-line
/// editing, not a one-line form).
///
/// `temp_path` is a scratch file *outside* the project, holding just
/// this item's commands, one per line -- opened in `Mode::Editing` like
/// any other file. Held alongside `menu` (the same "park the state,
/// hand it back once the editor really closes" shape
/// `AddUserMenuItem`/`ConfirmDiscard` already use) so
/// `editor_keymap::return_from_editor` can finish the edit
/// (`finish_command_edit`) once the editor session actually ends.
pub struct UserMenuCommandEdit {
    pub menu: UserMenuState,
    pub temp_path: PathBuf,
}

/// Writes `commands` (the selected item's own command lines) to a fresh
/// scratch file, one line per command, so `F4` can open it in the real
/// built-in editor instead of a bespoke UI form. The file lives in the
/// OS temp directory, not the project -- it's a working copy for this
/// one edit, never itself part of `LitastumMenu.toml`. A monotonic
/// counter (alongside the process id) keeps concurrent edits (or, more
/// realistically, concurrent tests) from colliding on the same path.
pub fn create_command_edit_file(commands: &[String]) -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("litastum-menu-command-{}-{n}.sh", std::process::id()));
    fs::write(&path, commands.join("\n"))?;
    Ok(path)
}

/// Finishes an `F4` command-edit session, once the real built-in editor
/// has genuinely closed (`editor_keymap::return_from_editor`): reads
/// back whatever `edit.temp_path` now contains (each non-blank line
/// becomes one command), replaces the selected item's commands with it
/// (`UserMenuState::replace_selected_commands`, which also persists),
/// and deletes the scratch file. Reading rather than trusting an
/// in-memory copy means this works the same whether the user actually
/// saved (`Ctrl+S`) or the "unsaved changes, discard?" prompt discarded
/// them -- either way the file on disk is the source of truth, same as
/// opening any other file in this editor. A file that couldn't be read
/// (deleted out from under us, or never wrote successfully) just leaves
/// the item's commands untouched, same "never block on a failed
/// read/write" rule the rest of this module follows.
pub fn finish_command_edit(edit: UserMenuCommandEdit) -> UserMenuState {
    let commands = fs::read_to_string(&edit.temp_path)
        .map(|content| content.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Err(err) = fs::remove_file(&edit.temp_path) {
        warn!(path = %edit.temp_path.display(), %err, "failed to remove the F4 command-edit scratch file");
    }

    let mut menu = edit.menu;
    menu.replace_selected_commands(commands);
    menu
}


/// `Mode::UserMenuPrompt`: collecting answers to a `Commands` item's
/// own `!?Label?Default!` placeholders (`parse::extract_prompts`)
/// before running it -- one text field shown at a time, in
/// first-appearance order, same as real Far Manager's own "each
/// distinct label prompts once" behavior (`parse::extract_prompts`'s
/// own doc comment).
pub struct UserMenuPromptState {
    /// The item's own commands, with `!&` already substituted --
    /// `!?Label?Default!` placeholders are still present until
    /// `accept_current` finishes collecting every answer.
    commands: Vec<String>,
    prompts: Vec<Prompt>,
    current: usize,
    answers: Vec<(String, String)>,
    pub value: String,
    pub cursor: usize,
    pub selection_anchor: Option<usize>,
}

impl UserMenuPromptState {
    /// `prompts` must be non-empty -- callers only build this once
    /// `parse::extract_prompts` on `commands` actually found at least
    /// one placeholder; a `Commands` item with none just runs directly
    /// instead (`explorer::user_menu::input`).
    pub fn new(commands: Vec<String>, prompts: Vec<Prompt>) -> Self {
        assert!(!prompts.is_empty(), "UserMenuPromptState needs at least one prompt to collect");
        let value = prompts[0].default.clone();
        let cursor = value.chars().count();
        Self { commands, prompts, current: 0, answers: Vec::new(), value, cursor, selection_anchor: None }
    }

    /// The label to show above the input field for whichever prompt is
    /// currently being asked.
    pub fn current_label(&self) -> &str {
        &self.prompts[self.current].label
    }

    /// How many prompts remain including this one, and the total --
    /// e.g. "2 of 3", for the popup's own title/footer.
    pub fn progress(&self) -> (usize, usize) {
        (self.current + 1, self.prompts.len())
    }

    /// Accepts the current field's value as this prompt's answer.
    /// Advances to the next prompt (pre-filling its own default) and
    /// returns `None` if there is one; once every prompt has an answer,
    /// substitutes them all into `commands` and returns the finished,
    /// ready-to-run list instead.
    pub fn accept_current(&mut self) -> Option<Vec<String>> {
        self.answers.push((self.prompts[self.current].label.clone(), self.value.clone()));
        self.current += 1;

        if self.current < self.prompts.len() {
            self.value = self.prompts[self.current].default.clone();
            self.cursor = self.value.chars().count();
            self.selection_anchor = None;
            None
        } else {
            Some(self.commands.iter().map(|command| parse::substitute_prompts(command, &self.answers)).collect())
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("user-menu")
    }

    mod resolve_tests {
        use super::*;

        #[test]
        fn not_found_when_neither_file_exists() {
            // `resolve_menu_with_fallback` with an explicit `None`, not
            // the public `resolve_menu` -- that goes through the real
            // config directory (`config_dir()`), which would make this
            // test's outcome depend on whatever `LitastumMenu.toml` a
            // developer running the suite actually happens to have
            // sitting there. See `resolve_menu`'s own doc comment.
            assert!(matches!(resolve_menu_with_fallback(&scratch_dir(), None), MenuFile::NotFound));
        }

        #[test]
        fn reads_an_existing_litastum_menu() {
            let dir = scratch_dir();
            fs::write(dir.join(OWN_FILE_NAME), "[[item]]\ntitle = \"status\"\nhotkey = \"s\"\ncommands = [\"git status -s\"]\n").unwrap();

            let MenuFile::Own(_, items) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };

            assert_eq!(items.len(), 1);
            assert_eq!(items[0].title, "status");
        }

        /// Regression coverage for the real request: dropping a
        /// `FarMenu.ini` into a directory that already has a configured
        /// `LitastumMenu.toml` used to be silently ignored (the old
        /// `Own`-wins precedence). It should now still surface the
        /// choice -- `FarMenuFound`, not `Own` -- so porting can
        /// deliberately overwrite (with a backup) an already-configured
        /// menu.
        #[test]
        fn a_far_menu_takes_priority_over_an_existing_litastum_menu() {
            let dir = scratch_dir();
            fs::write(dir.join(OWN_FILE_NAME), "[[item]]\ntitle = \"mine\"\ncommands = [\"echo mine\"]\n").unwrap();
            fs::write(dir.join(FAR_FILE_NAME), "s: theirs\necho theirs\n").unwrap();

            let result = resolve_menu(&dir);

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == dir.join(FAR_FILE_NAME)));
        }

        /// The whole point of the port-on-confirm flow: a real
        /// `FarMenu.ini` should be *found*, not read directly or
        /// silently converted -- `resolve_menu` only reports it, leaving the
        /// actual conversion to `port_far_menu` once confirmed.
        #[test]
        fn reports_a_far_menu_without_reading_or_converting_it() {
            let dir = scratch_dir();
            fs::write(dir.join(FAR_FILE_NAME), "s: status\ngit status -s\n").unwrap();

            let result = resolve_menu(&dir);

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == dir.join(FAR_FILE_NAME)));
            assert!(!dir.join(OWN_FILE_NAME).exists(), "resolve alone must not create LitastumMenu.toml");
        }
    }

    /// The actual point of this whole change: a menu set up once should
    /// be reachable from any directory, not just the one it was created
    /// in -- regression coverage for the real report (switching to
    /// another directory made the menu unreadable). Exercises
    /// `resolve_menu_with_fallback` directly with two plain scratch
    /// directories standing in for "active panel dir" / "common config
    /// dir", rather than the public `resolve_menu` -- see
    /// `not_found_when_neither_file_exists`'s own comment on why the
    /// real OS config directory isn't touched by these tests.
    mod common_fallback_tests {
        use super::*;

        #[test]
        fn falls_back_to_the_common_menu_when_the_active_directory_has_none() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"status\"\ncommands = [\"git status -s\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the common directory")
            };

            assert_eq!(menu_dir, common, "edits should persist back to the common directory, not the active one");
            assert_eq!(items[0].title, "status");
        }

        #[test]
        fn a_local_menu_wins_over_the_common_one() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), "[[item]]\ntitle = \"local\"\ncommands = [\"echo local\"]\n").unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the active directory")
            };

            assert_eq!(menu_dir, active);
            assert_eq!(items[0].title, "local");
        }

        #[test]
        fn a_local_far_menu_ini_still_wins_over_a_common_litastum_menu() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(FAR_FILE_NAME), "s: theirs\necho theirs\n").unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let result = resolve_menu_with_fallback(&active, Some(&common));

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == active.join(FAR_FILE_NAME)));
        }

        #[test]
        fn a_common_far_menu_ini_is_offered_too_once_the_active_directory_has_nothing() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(common.join(FAR_FILE_NAME), "s: status\ngit status -s\n").unwrap();

            let result = resolve_menu_with_fallback(&active, Some(&common));

            assert!(matches!(result, MenuFile::FarMenuFound(path) if path == common.join(FAR_FILE_NAME)));
        }

        #[test]
        fn not_found_when_neither_directory_has_anything() {
            assert!(matches!(resolve_menu_with_fallback(&scratch_dir(), Some(&scratch_dir())), MenuFile::NotFound));
        }

        /// Regression coverage for the actual real-world report: `F2`
        /// pressed once in a directory before this fallback existed
        /// left behind an empty, commented-out-only `LitastumMenu.toml`
        /// there (`create_menu_file`'s own template) -- that stray local
        /// file should not permanently block the common menu from ever
        /// being consulted for that directory.
        #[test]
        fn an_empty_local_template_falls_through_to_the_common_menu() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), EMPTY_MENU_TEMPLATE).unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the common directory")
            };

            assert_eq!(menu_dir, common);
            assert_eq!(items[0].title, "common");
        }

        /// The flip side: with no common menu (or none configured) to
        /// fall through to, the empty local file is still what gets
        /// shown -- not `NotFound`, which would make `open_user_menu`
        /// silently overwrite it via `create_menu_file` on every `F2`.
        #[test]
        fn an_empty_local_template_is_still_shown_when_there_is_nothing_to_fall_back_to() {
            let active = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), EMPTY_MENU_TEMPLATE).unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, None) else {
                panic!("expected MenuFile::Own from the active directory")
            };

            assert_eq!(menu_dir, active);
            assert!(items.is_empty());
        }

        /// A local menu with at least one real item still wins over the
        /// common one -- only a genuinely *empty* local file falls
        /// through, per the two tests above.
        #[test]
        fn a_non_empty_local_menu_still_wins_over_the_common_one() {
            let active = scratch_dir();
            let common = scratch_dir();
            fs::write(active.join(OWN_FILE_NAME), "[[item]]\ntitle = \"local\"\ncommands = [\"echo local\"]\n").unwrap();
            fs::write(common.join(OWN_FILE_NAME), "[[item]]\ntitle = \"common\"\ncommands = [\"echo common\"]\n").unwrap();

            let MenuFile::Own(menu_dir, items) = resolve_menu_with_fallback(&active, Some(&common)) else {
                panic!("expected MenuFile::Own from the active directory")
            };

            assert_eq!(menu_dir, active);
            assert_eq!(items[0].title, "local");
        }
    }

    mod port_far_menu_tests {
        use super::*;

        /// The actual point of porting: a real `FarMenu.ini` becomes a
        /// working `LitastumMenu.toml` alongside it.
        #[test]
        fn ports_a_far_menu_into_a_working_litastum_toml() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let far_content = "s: status\ngit status -s\n\nc: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n";
            fs::write(&far_path, far_content).unwrap();

            let items = port_far_menu(&far_path);

            assert_eq!(items.len(), 2);
            assert_eq!(items[0].title, "status");

            let MenuFile::Own(_, reread) = resolve_menu(&dir) else { panic!("expected the ported LitastumMenu.toml to now resolve") };
            assert_eq!(reread, items);
        }

        /// Regression coverage for the real request: once ported,
        /// `FarMenu.ini` has to move out of the way -- `resolve_menu`
        /// now reports its mere presence unconditionally, so leaving it
        /// in place (the very first version of this feature's own
        /// behavior) would re-trigger the same prompt on every future
        /// `F2`/startup check.
        #[test]
        fn moves_far_menu_ini_aside_after_porting_it() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let far_content = "s: status\ngit status -s\n";
            fs::write(&far_path, far_content).unwrap();

            port_far_menu(&far_path);

            assert!(!far_path.exists(), "FarMenu.ini should have been moved aside");
            let backup_content = fs::read_to_string(dir.join("FarMenu.ini.bak")).unwrap();
            assert_eq!(backup_content, far_content);
            assert!(matches!(resolve_menu(&dir), MenuFile::Own(_, _)), "should no longer be re-detected as a FarMenu.ini to port");
        }

        /// Regression coverage for the actual real-world report: a
        /// `FarMenu.ini` exported straight from a real Far Manager
        /// install ported to zero items -- confirmed by inspecting its
        /// raw bytes directly, it's UTF-16LE with a BOM (`FF FE`, every
        /// ASCII character null-padded), which a strict-UTF-8 read
        /// fails on outright. Built here the same way a real text
        /// editor saving "UTF-16 LE" would produce it, not by guessing
        /// at the byte layout.
        #[test]
        fn ports_a_real_utf16le_far_menu_ini() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let text = "s: status\r\ngit status -s\r\n";
            let mut bytes = vec![0xFF, 0xFE];
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            fs::write(&far_path, &bytes).unwrap();

            let items = port_far_menu(&far_path);

            assert_eq!(items.len(), 1, "should have actually parsed the UTF-16LE content, not silently produced nothing");
            assert_eq!(items[0].title, "status");
            assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
        }

        /// Regression coverage for the actual "what if I already have a
        /// menu" scenario this whole change was requested for: porting
        /// over an existing `LitastumMenu.toml` must not silently
        /// discard it.
        #[test]
        fn backs_up_an_existing_litastum_menu_before_overwriting_it() {
            let dir = scratch_dir();
            let own_path = dir.join(OWN_FILE_NAME);
            let old_content = "[[item]]\ntitle = \"old\"\ncommands = [\"echo old\"]\n";
            fs::write(&own_path, old_content).unwrap();
            let far_path = dir.join(FAR_FILE_NAME);
            fs::write(&far_path, "s: new\necho new\n").unwrap();

            let items = port_far_menu(&far_path);

            assert_eq!(items[0].title, "new", "the freshly ported menu should be what's active now");
            let backup_content = fs::read_to_string(dir.join("LitastumMenu.toml.bak")).unwrap();
            assert_eq!(backup_content, old_content, "the old menu should be preserved as a backup, not lost");
        }
    }

    mod backup_far_menu_without_porting_tests {
        use super::*;

        #[test]
        fn moves_far_menu_ini_to_a_backup_and_returns_its_path() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let content = "s: status\ngit status -s\n";
            fs::write(&far_path, content).unwrap();

            let backup = backup_far_menu_without_porting(&far_path).unwrap();

            assert_eq!(backup, dir.join("FarMenu.ini.bak"));
            assert!(!far_path.exists());
            assert_eq!(fs::read_to_string(&backup).unwrap(), content);
        }

        #[test]
        fn does_not_touch_any_existing_litastum_menu() {
            let dir = scratch_dir();
            let own_path = dir.join(OWN_FILE_NAME);
            fs::write(&own_path, "[[item]]\ntitle = \"mine\"\ncommands = [\"echo mine\"]\n").unwrap();
            let far_path = dir.join(FAR_FILE_NAME);
            fs::write(&far_path, "s: theirs\necho theirs\n").unwrap();

            backup_far_menu_without_porting(&far_path);

            let MenuFile::Own(_, items) = resolve_menu(&dir) else { panic!("expected MenuFile::Own now that FarMenu.ini is gone") };
            assert_eq!(items[0].title, "mine");
        }
    }

    mod read_text_file_any_encoding_tests {
        use super::*;

        fn utf16_bytes(text: &str, bom: [u8; 2], to_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
            let mut bytes = bom.to_vec();
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&to_bytes(unit));
            }
            bytes
        }

        #[test]
        fn reads_plain_utf8() {
            let dir = scratch_dir();
            let path = dir.join("plain.txt");
            fs::write(&path, "hello").unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello");
        }

        #[test]
        fn strips_a_utf8_bom() {
            let dir = scratch_dir();
            let path = dir.join("bom.txt");
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(b"hello");
            fs::write(&path, &bytes).unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello");
        }

        /// The actual real-world case: real Far Manager's own default
        /// `FarMenu.ini` encoding.
        #[test]
        fn reads_utf16_le_with_bom() {
            let dir = scratch_dir();
            let path = dir.join("utf16le.ini");
            fs::write(&path, utf16_bytes("hello \u{416}", [0xFF, 0xFE], u16::to_le_bytes)).unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello \u{416}");
        }

        #[test]
        fn reads_utf16_be_with_bom() {
            let dir = scratch_dir();
            let path = dir.join("utf16be.ini");
            fs::write(&path, utf16_bytes("hello \u{416}", [0xFE, 0xFF], u16::to_be_bytes)).unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello \u{416}");
        }

        #[test]
        fn errors_on_genuinely_invalid_utf8_with_no_bom() {
            let dir = scratch_dir();
            let path = dir.join("invalid.ini");
            fs::write(&path, [0xFF, 0x00, 0x01]).unwrap(); // 0xFF alone (no matching 0xFE) is invalid UTF-8
            assert!(read_text_file_any_encoding(&path).is_err());
        }
    }

    mod create_menu_file_tests {
        use super::*;

        #[test]
        fn creates_a_litastum_menu_file() {
            let dir = scratch_dir();

            let path = create_menu_file(&dir).unwrap();

            assert_eq!(path, dir.join(OWN_FILE_NAME));
            assert!(path.is_file());
        }

        #[test]
        fn the_created_file_is_then_found_by_resolve_menu() {
            let dir = scratch_dir();
            create_menu_file(&dir).unwrap();

            let MenuFile::Own(_, items) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };
            assert!(items.is_empty(), "the template is all comments, so no real items yet");
        }

        /// Regression coverage for `open_user_menu` now targeting the
        /// common config directory on a fresh `F2` instead of the
        /// active one: that directory (`%APPDATA%\litastum\` or
        /// equivalent) may not exist yet at all on a machine that has
        /// never saved a theme/setup -- a plain `fs::write` would fail
        /// outright with its parent missing, so `create_menu_file` now
        /// `create_dir_all`s first.
        #[test]
        fn creates_the_directory_itself_if_it_does_not_exist_yet() {
            let dir = scratch_dir().join("not-created-yet");
            assert!(!dir.exists());

            let path = create_menu_file(&dir).unwrap();

            assert!(dir.is_dir());
            assert!(path.is_file());
        }
    }

    mod user_menu_state_tests {
        use super::*;

        fn menu_with(items: Vec<MenuItem>) -> UserMenuState {
            UserMenuState::from_items(scratch_dir(), items)
        }

        fn command_item(title: &str) -> MenuItem {
            MenuItem { hotkey: None, title: title.to_string(), body: MenuItemBody::Commands(vec!["echo hi".to_string()]) }
        }

        fn command_item_with_hotkey(hotkey: char, title: &str) -> MenuItem {
            MenuItem { hotkey: Some(hotkey), title: title.to_string(), body: MenuItemBody::Commands(vec!["echo hi".to_string()]) }
        }

        fn submenu_item(title: &str, children: Vec<MenuItem>) -> MenuItem {
            MenuItem { hotkey: None, title: title.to_string(), body: MenuItemBody::Submenu(children) }
        }

        #[test]
        fn move_down_and_up_clamp_at_the_edges() {
            let mut menu = menu_with(vec![command_item("a"), command_item("b")]);
            menu.move_down();
            assert_eq!(menu.current_level().selected, 1);
            menu.move_down();
            assert_eq!(menu.current_level().selected, 1, "clamped at the last item");
            menu.move_up();
            menu.move_up();
            assert_eq!(menu.current_level().selected, 0, "clamped at the first item");
        }

        /// Regression coverage for the real report: `hotkey` used to be
        /// parsed and shown but never actually wired up to a key at
        /// all.
        #[test]
        fn select_by_hotkey_moves_the_cursor_to_the_matching_item() {
            let mut menu = menu_with(vec![command_item_with_hotkey('s', "status"), command_item_with_hotkey('c', "commit")]);

            assert!(menu.select_by_hotkey('c'));

            assert_eq!(menu.current_level().selected, 1);
        }

        #[test]
        fn select_by_hotkey_is_case_insensitive() {
            let mut menu = menu_with(vec![command_item_with_hotkey('s', "status")]);

            assert!(menu.select_by_hotkey('S'));

            assert_eq!(menu.current_level().selected, 0);
        }

        #[test]
        fn select_by_hotkey_returns_false_and_leaves_the_cursor_alone_when_unmatched() {
            let mut menu = menu_with(vec![command_item_with_hotkey('s', "status"), command_item_with_hotkey('c', "commit")]);
            menu.move_down();

            assert!(!menu.select_by_hotkey('z'));

            assert_eq!(menu.current_level().selected, 1, "cursor should be unchanged");
        }

        /// The whole point of scoping the lookup to `current_items()`:
        /// a hotkey only ever matches within whatever level is actually
        /// on screen right now, same as real Far Manager -- a
        /// collapsed submenu's own items aren't searched.
        #[test]
        fn select_by_hotkey_only_searches_the_current_level_not_a_collapsed_submenu() {
            let mut menu = menu_with(vec![submenu_item("parent", vec![command_item_with_hotkey('c', "child")])]);

            assert!(!menu.select_by_hotkey('c'), "the child's hotkey shouldn't match while its submenu is still collapsed");

            menu.enter_submenu();
            assert!(menu.select_by_hotkey('c'), "but does once actually inside that submenu");
        }

        #[test]
        fn enter_submenu_descends_and_back_restores_the_parent_level() {
            let mut menu = menu_with(vec![submenu_item("parent", vec![command_item("child")])]);

            assert!(menu.enter_submenu());
            assert_eq!(menu.current_level().items[0].title, "child");

            let stayed_in_menu = menu.back();
            assert!(stayed_in_menu);
            assert_eq!(menu.current_level().items[0].title, "parent");
        }

        #[test]
        fn enter_submenu_is_a_noop_on_a_commands_item() {
            let mut menu = menu_with(vec![command_item("leaf")]);
            assert!(!menu.enter_submenu());
        }

        #[test]
        fn back_at_the_top_level_signals_close() {
            let mut menu = menu_with(vec![command_item("a")]);
            assert!(!menu.back());
        }

        #[test]
        fn entering_a_submenu_preserves_the_parent_levels_own_cursor_position() {
            let mut menu = menu_with(vec![command_item("a"), submenu_item("b", vec![command_item("child")])]);
            menu.move_down(); // cursor on "b"

            menu.enter_submenu();
            menu.back();

            assert_eq!(menu.current_level().selected, 1, "should still be on \"b\", not reset to 0");
        }

        #[test]
        fn insert_item_adds_right_after_the_selected_item_and_selects_it() {
            let mut menu = menu_with(vec![command_item("a"), command_item("b")]);

            menu.insert_item(command_item("new"));

            let level = menu.current_level();
            assert_eq!(level.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["a", "new", "b"]);
            assert_eq!(level.selected, 1, "the newly inserted item should be selected");
        }

        #[test]
        fn insert_item_into_an_empty_level_works() {
            let mut menu = menu_with(Vec::new());

            menu.insert_item(command_item("first"));

            assert_eq!(menu.current_level().items.len(), 1);
            assert_eq!(menu.current_level().selected, 0);
        }

        #[test]
        fn delete_selected_removes_the_highlighted_item_and_clamps_the_cursor() {
            let mut menu = menu_with(vec![command_item("a"), command_item("b")]);
            menu.move_down(); // selected on "b" (last)

            menu.delete_selected();

            assert_eq!(menu.current_level().items.len(), 1);
            assert_eq!(menu.current_level().items[0].title, "a");
            assert_eq!(menu.current_level().selected, 0, "should clamp back onto the remaining item");
        }

        #[test]
        fn delete_selected_on_an_empty_level_does_not_panic() {
            let mut menu = menu_with(Vec::new());
            menu.delete_selected();
            assert!(menu.current_level().items.is_empty());
        }

        /// The actual point of `F4`'s new behavior: replacing just the
        /// highlighted item's own commands, title untouched, persisted.
        #[test]
        fn replace_selected_commands_updates_the_item_and_persists() {
            let dir = scratch_dir();
            let mut menu = UserMenuState::from_items(dir.clone(), vec![command_item("status")]);

            menu.replace_selected_commands(vec!["git status -sb".to_string(), "echo done".to_string()]);

            assert_eq!(menu.current_level().items[0].title, "status", "title should be untouched");
            assert_eq!(menu.current_level().items[0].body, MenuItemBody::Commands(vec!["git status -sb".to_string(), "echo done".to_string()]));
            let MenuFile::Own(_, reread) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };
            assert_eq!(reread[0].body, MenuItemBody::Commands(vec!["git status -sb".to_string(), "echo done".to_string()]));
        }

        #[test]
        fn replace_selected_commands_is_a_noop_on_a_submenu_item() {
            let mut menu = menu_with(vec![submenu_item("parent", vec![command_item("child")])]);

            menu.replace_selected_commands(vec!["echo nope".to_string()]);

            assert_eq!(menu.current_level().items[0].body, MenuItemBody::Submenu(vec![command_item("child")]));
        }

        #[test]
        fn replace_selected_commands_on_an_empty_level_does_not_panic() {
            let mut menu = menu_with(Vec::new());
            menu.replace_selected_commands(vec!["echo nope".to_string()]);
            assert!(menu.current_level().items.is_empty());
        }

        #[test]
        fn replace_selected_commands_with_an_empty_list_is_a_noop() {
            let mut menu = menu_with(vec![command_item("status")]);

            menu.replace_selected_commands(Vec::new());

            assert_eq!(menu.current_level().items[0].body, MenuItemBody::Commands(vec!["echo hi".to_string()]));
        }

        /// The actual point of the whole rewrite from a stack-of-clones
        /// to a single canonical tree: editing *inside a nested
        /// submenu* must be visible after backing out of it, and must
        /// make it into the persisted file -- neither was true when
        /// `enter_submenu` cloned children into a disposable level.
        #[test]
        fn editing_inside_a_nested_submenu_persists_and_survives_navigating_back_out() {
            let dir = scratch_dir();
            let mut menu = UserMenuState::from_items(dir.clone(), vec![submenu_item("parent", vec![command_item("child")])]);

            menu.enter_submenu();
            menu.insert_item(command_item("new sibling"));
            menu.back();

            // Re-enter and check the edit is still there in memory...
            menu.enter_submenu();
            let titles: Vec<&str> = menu.current_level().items.iter().map(|i| i.title.as_str()).collect();
            assert_eq!(titles, vec!["child", "new sibling"]);

            // ...and that it was actually written to disk, not just
            // held in the in-memory clone the old design would have
            // silently discarded.
            let MenuFile::Own(_, reread) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };
            let MenuItemBody::Submenu(children) = &reread[0].body else { panic!("expected the top item to still be a submenu") };
            assert_eq!(children.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["child", "new sibling"]);
        }
    }

    mod add_user_menu_item_state_tests {
        use super::*;

        #[test]
        fn advance_from_title_moves_to_the_command_stage() {
            let mut form = AddUserMenuItemState::new();
            form.title = "status".to_string();

            assert!(form.advance_from_title());
            assert!(!form.is_title_stage());
        }

        #[test]
        fn advance_from_title_refuses_a_blank_title() {
            let mut form = AddUserMenuItemState::new();
            form.title = "   ".to_string();

            assert!(!form.advance_from_title());
            assert!(form.is_title_stage(), "should stay on the title stage");
        }

        #[test]
        fn finish_with_a_command_builds_a_leaf_item() {
            let mut form = AddUserMenuItemState::new();
            form.title = "status".to_string();
            form.command = "git status -s".to_string();

            let item = form.finish();

            assert_eq!(item.title, "status");
            assert_eq!(item.hotkey, None);
            assert_eq!(item.body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
        }

        #[test]
        fn finish_with_no_command_builds_an_empty_submenu() {
            let mut form = AddUserMenuItemState::new();
            form.title = "git".to_string();

            let item = form.finish();

            assert_eq!(item.body, MenuItemBody::Submenu(Vec::new()));
        }

        #[test]
        fn finish_trims_the_title_and_command() {
            let mut form = AddUserMenuItemState::new();
            form.title = "  status  ".to_string();
            form.command = "  git status -s  ".to_string();

            let item = form.finish();

            assert_eq!(item.title, "status");
            assert_eq!(item.body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
        }
    }

    mod command_edit_file_tests {
        use super::*;

        #[test]
        fn create_command_edit_file_writes_one_command_per_line() {
            let path = create_command_edit_file(&["git status -s".to_string(), "echo done".to_string()]).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), "git status -s\necho done");
            fs::remove_file(&path).unwrap();
        }

        /// The actual point of the whole feature: `F4` opens a real
        /// editor session on a scratch file, and closing it feeds
        /// whatever's now in that file back into the item -- title
        /// untouched, persisted, scratch file cleaned up.
        #[test]
        fn finish_command_edit_applies_the_files_current_contents_and_deletes_it() {
            let dir = scratch_dir();
            let menu = UserMenuState::from_items(dir.clone(), parse::parse(": status\necho hi\n"));
            let temp_path = create_command_edit_file(&["echo hi".to_string()]).unwrap();
            fs::write(&temp_path, "git status -sb\necho done\n").unwrap(); // simulates an edit + save

            let menu = finish_command_edit(UserMenuCommandEdit { menu, temp_path: temp_path.clone() });

            assert_eq!(menu.current_level().items[0].title, "status", "title should be untouched");
            assert_eq!(menu.current_level().items[0].body, MenuItemBody::Commands(vec!["git status -sb".to_string(), "echo done".to_string()]));
            assert!(!temp_path.exists(), "the scratch file should have been cleaned up");
            let MenuFile::Own(_, reread) = resolve_menu(&dir) else { panic!("expected MenuFile::Own") };
            assert_eq!(reread[0].body, MenuItemBody::Commands(vec!["git status -sb".to_string(), "echo done".to_string()]));
        }

        /// An unmodified (or emptied-and-discarded) scratch file just
        /// re-applies the same commands it started with -- covers the
        /// "closed without saving" path, where the file on disk never
        /// changed from what `create_command_edit_file` wrote.
        #[test]
        fn finish_command_edit_with_an_unmodified_file_leaves_commands_unchanged() {
            let dir = scratch_dir();
            let menu = UserMenuState::from_items(dir.clone(), parse::parse(": status\necho hi\n"));
            let temp_path = create_command_edit_file(&["echo hi".to_string()]).unwrap();

            let menu = finish_command_edit(UserMenuCommandEdit { menu, temp_path });

            assert_eq!(menu.current_level().items[0].body, MenuItemBody::Commands(vec!["echo hi".to_string()]));
        }
    }

    mod user_menu_prompt_state_tests {
        use super::*;

        #[test]
        fn starts_prefilled_with_the_first_prompts_default() {
            let prompts = vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }];
            let state = UserMenuPromptState::new(vec!["git checkout !?Branch?Master!".to_string()], prompts);
            assert_eq!(state.value, "Master");
            assert_eq!(state.current_label(), "Branch");
            assert_eq!(state.progress(), (1, 1));
        }

        #[test]
        fn accept_current_advances_to_the_next_prompt() {
            let prompts = vec![
                Prompt { label: "First".to_string(), default: "a".to_string() },
                Prompt { label: "Second".to_string(), default: "b".to_string() },
            ];
            let mut state = UserMenuPromptState::new(vec!["echo !?First?a! !?Second?b!".to_string()], prompts);

            let result = state.accept_current();

            assert_eq!(result, None, "should not finish yet -- one prompt left");
            assert_eq!(state.current_label(), "Second");
            assert_eq!(state.value, "b", "pre-filled with the next prompt's own default");
        }

        #[test]
        fn accept_current_returns_the_substituted_commands_once_every_prompt_is_answered() {
            let prompts = vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }];
            let mut state = UserMenuPromptState::new(vec!["git checkout !?Branch?Master!".to_string()], prompts);
            state.value = "feature/x".to_string();

            let result = state.accept_current();

            assert_eq!(result, Some(vec!["git checkout feature/x".to_string()]));
        }
    }
}
