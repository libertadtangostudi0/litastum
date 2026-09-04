use color_eyre::eyre::Result;

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::keymap::Command;
use crate::menu::MainMenu;


/// Executes a resolved `Command` against the app state. This is the one
/// chokepoint keyboard input goes through — and, per the roadmap's
/// stage 4, where scripts will funnel their commands too, instead of
/// touching the filesystem/process directly.
pub fn execute(command: Command, app: &mut App) -> Result<()> {
    match command {
        Command::MoveUp => app.active_panel().move_up(),
        Command::MoveDown => app.active_panel().move_down(),
        Command::MoveLeft => app.active_panel().move_left(),
        Command::MoveRight => app.active_panel().move_right(),
        Command::EnterSelected => app.active_panel().enter_selected()?,
        Command::ToggleActive => app.toggle_active(),
        Command::EditSelected => open_editor(app),
        Command::OpenMenu => app.mode = Mode::MainMenu(MainMenu::open()),
        Command::Quit => app.should_quit = true,
    }
    Ok(())
}


/// Opens the file under the cursor in the built-in editor (`editor.rs`,
/// backed by `edtui`). Does nothing for directories, and for files that
/// fail to load as UTF-8 text (binary files aren't supported yet — see
/// `TODO.md`) rather than crashing the app.
fn open_editor(app: &mut App) {
    let Some(path) = app.active_panel().selected_path() else {
        return;
    };
    if path.is_dir() {
        return;
    }

    let syntax_theme = app.syntax_theme.clone();
    if let Ok(editor) = Editor::open(path, syntax_theme) {
        app.mode = Mode::Editing(editor);
    }
}
