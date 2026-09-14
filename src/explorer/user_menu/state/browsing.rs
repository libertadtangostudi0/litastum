use std::fs;
use std::path::PathBuf;

use tracing::warn;

use super::OWN_FILE_NAME;
use crate::explorer::user_menu::parse::{MenuItem, MenuItemBody};
use crate::explorer::user_menu::toml_format;

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
    /// resolution itself lives in `resolve_menu`/`port_far_menu`
    /// (`resolve.rs`/`porting.rs`), kept separate so
    /// `explorer::command::open_user_menu` can decide what to do
    /// (browse, offer to port, or create) before ever building one of
    /// these. `dir` is where edits get persisted back to
    /// (`LitastumMenu.toml`), independent of wherever `items` itself
    /// originally came from (a fresh read, or a just-completed port).
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::user_menu::state::scratch_dir;
    use crate::explorer::user_menu::state::{resolve_menu, MenuFile};

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
