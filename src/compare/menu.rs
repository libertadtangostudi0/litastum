use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Overlay};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};

use super::line_ending_menu::open_compare_line_ending_menu;

/// An item of Compare's own F9 menu -- the same shape as the editor's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareMenuItem {
    LineEndings,
}


impl CompareMenuItem {
    pub const ALL: [Self; 1] = [Self::LineEndings];


    pub fn label(self) -> &'static str {
        match self {
            Self::LineEndings => "Line endings",
        }
    }
}


pub type CompareMenu = ChoiceMenu<CompareMenuItem>;


pub fn open_compare_menu() -> CompareMenu {
    ChoiceMenu::new(CompareMenuItem::ALL, None)
}


/// `Enter` on `Line endings` opens that picker in its place; `Esc`
/// closes.
pub fn handle_compare_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(Overlay::CompareMenu(menu)) = &mut app.overlay else {
        return Ok(());
    };

    let outcome = menu.handle_key(key);
    debug!(?key, ?outcome, "compare menu key");
    app.overlay = match outcome {
        MenuOutcome::Open => return Ok(()),
        MenuOutcome::Chosen(CompareMenuItem::LineEndings) => Some(Overlay::CompareLineEndingMenu(open_compare_line_ending_menu(app.compare_line_ending_display))),
        MenuOutcome::Closed => None,
    };
    Ok(())
}
