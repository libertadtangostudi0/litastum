use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};
use crate::command_line::Effect;

use super::LineEnding;

/// The editor's F9 -> Line endings picker, opened on the line break a
/// save would write.
pub type EditorLineEndingMenu = ChoiceMenu<LineEnding>;


pub fn open_line_ending_menu(current: LineEnding) -> EditorLineEndingMenu {
    ChoiceMenu::new(LineEnding::ALL, Some(current))
}


/// `Enter` picks the highlighted line break for the editor underneath
/// (`Editor::set_line_ending`: the next save writes it throughout);
/// `Esc` closes without changing anything.
pub fn handle_line_ending_menu_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::EditorLineEndingMenu(menu)) = &mut app.overlay else {
        return Ok(Effect::None);
    };

    let outcome = menu.handle_key(key);
    debug!(key = ?crate::logging::LoggedKey(key), ?outcome, "editor line ending menu key");
    if outcome == MenuOutcome::Open {
        return Ok(Effect::None);
    }

    app.overlay = None;
    if let (MenuOutcome::Chosen(ending), Mode::Editing(editor)) = (outcome, &mut app.mode) {
        debug!(?ending, "editor line endings picked");
        editor.set_line_ending(ending);
    }
    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{editing_app, key};

    fn app_with_menu(text: &str) -> App {
        let mut app = editing_app(text, EditorKeymapMode::Standard);
        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        app.overlay = Some(Overlay::EditorLineEndingMenu(open_line_ending_menu(editor.line_ending())));
        app
    }

    fn editor(app: &App) -> &crate::editor::Editor {
        let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
        editor
    }

    #[test]
    fn opens_on_the_files_own_line_break() {
        let app = app_with_menu("a\r\nb\r\n");
        let Some(Overlay::EditorLineEndingMenu(menu)) = &app.overlay else { panic!("expected the picker") };
        assert_eq!(menu.selected(), LineEnding::CrLf);
    }

    #[test]
    fn enter_picks_the_highlighted_one_and_marks_the_file_modified() {
        let mut app = app_with_menu("a\r\nb\r\n");
        handle_line_ending_menu_key(&mut app, key(KeyCode::Up)).unwrap();

        handle_line_ending_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.overlay.is_none());
        assert_eq!(editor(&app).line_ending(), LineEnding::Lf);
        assert_eq!(editor(&app).line_ending_label(), "LF");
        assert!(editor(&app).is_dirty());
    }

    #[test]
    fn esc_closes_without_changing_anything() {
        let mut app = app_with_menu("a\r\nb\r\n");
        handle_line_ending_menu_key(&mut app, key(KeyCode::Up)).unwrap();

        handle_line_ending_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none());
        assert_eq!(editor(&app).line_ending(), LineEnding::CrLf);
        assert!(!editor(&app).is_dirty());
    }

    #[test]
    fn is_a_noop_without_the_picker_open() {
        let mut app = app_with_menu("a\nb\n");
        app.overlay = None;

        handle_line_ending_menu_key(&mut app, key(KeyCode::Down)).unwrap();
        handle_line_ending_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(editor(&app).line_ending(), LineEnding::Lf);
    }
}
