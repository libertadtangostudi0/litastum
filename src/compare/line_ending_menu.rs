use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};
use crate::theming::config;

use super::line_ending::LineEndingDisplay;

/// Compare's F9 -> Line endings picker, opened on the active choice.
pub type CompareLineEndingMenu = ChoiceMenu<LineEndingDisplay>;


pub fn open_compare_line_ending_menu(current: LineEndingDisplay) -> CompareLineEndingMenu {
    ChoiceMenu::new(LineEndingDisplay::all(), Some(current))
}


/// `Enter` applies the highlighted choice live (`App::compare_line_ending_display`,
/// read by `ui/compare.rs` every frame) and persists it; `Esc` closes
/// without changing anything. Either way, back to `Mode::CompareFiles`
/// with the same `CompareState`.
pub fn handle_compare_line_ending_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::CompareLineEndingMenu(_, menu) = &mut app.mode else {
        return Ok(());
    };

    let outcome = menu.handle_key(key);
    debug!(?key, ?outcome, "compare line ending menu key");
    if outcome == MenuOutcome::Open {
        return Ok(());
    }

    let Mode::CompareLineEndingMenu(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::CompareLineEndingMenu above");
    };
    if let MenuOutcome::Chosen(display) = outcome {
        app.compare_line_ending_display = display;
        config::set_compare_line_ending_display(display);
    }
    app.mode = Mode::CompareFiles(state);
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_the_current_display() {
        assert_eq!(open_compare_line_ending_menu(LineEndingDisplay::Shown).selected(), LineEndingDisplay::Shown);
    }
}
