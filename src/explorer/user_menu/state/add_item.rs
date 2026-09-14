use crate::explorer::user_menu::parse::{MenuItem, MenuItemBody};

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


#[cfg(test)]
mod tests {
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
