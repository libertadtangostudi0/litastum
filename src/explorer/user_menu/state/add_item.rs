use crate::explorer::user_menu::parse::{MenuItem, MenuItemBody};
use crate::text_field::TextField;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddItemStage {
    Title,
    Command,
}

/// `Overlay::AddUserMenuItem`: a small two-field form (`Ins` while
/// browsing the user menu) for adding a new item without leaving the
/// popup to hand-edit `LitastumMenu.toml`. Deliberately minimal -- no
/// hotkey field, no multi-command items, no authoring help for `!&`/
/// `!?Label?Default!` -- those are still easiest to add by hand-editing
/// the file afterward; this covers the common case (one title, one
/// command, or a bare submenu to build out by entering it and adding
/// more items the same way).
pub struct AddUserMenuItemState {
    stage: AddItemStage,
    pub title: TextField,
    pub command: TextField,
}

impl AddUserMenuItemState {
    pub fn new() -> Self {
        Self {
            stage: AddItemStage::Title,
            title: TextField::new(),
            command: TextField::new(),
        }
    }

    pub fn is_title_stage(&self) -> bool {
        self.stage == AddItemStage::Title
    }

    /// `Enter` on the title field -- advances to the command field if
    /// the title isn't blank (`true`), a no-op otherwise (`false`):
    /// there's nothing sensible to call a titleless menu item.
    pub fn advance_from_title(&mut self) -> bool {
        if self.title.text().trim().is_empty() {
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
        let title = self.title.text().trim().to_string();
        let command = self.command.text().trim();
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
        form.title.set_text("status");

        assert!(form.advance_from_title());
        assert!(!form.is_title_stage());
    }

    #[test]
    fn advance_from_title_refuses_a_blank_title() {
        let mut form = AddUserMenuItemState::new();
        form.title.set_text("   ");

        assert!(!form.advance_from_title());
        assert!(form.is_title_stage(), "should stay on the title stage");
    }

    #[test]
    fn finish_with_a_command_builds_a_leaf_item() {
        let mut form = AddUserMenuItemState::new();
        form.title.set_text("status");
        form.command.set_text("git status -s");

        let item = form.finish();

        assert_eq!(item.title, "status");
        assert_eq!(item.hotkey, None);
        assert_eq!(item.body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
    }

    #[test]
    fn finish_with_no_command_builds_an_empty_submenu() {
        let mut form = AddUserMenuItemState::new();
        form.title.set_text("git");

        let item = form.finish();

        assert_eq!(item.body, MenuItemBody::Submenu(Vec::new()));
    }

    #[test]
    fn finish_trims_the_title_and_command() {
        let mut form = AddUserMenuItemState::new();
        form.title.set_text("  status  ");
        form.command.set_text("  git status -s  ");

        let item = form.finish();

        assert_eq!(item.title, "status");
        assert_eq!(item.body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
    }
}
