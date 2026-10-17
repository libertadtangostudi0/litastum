//! What the terminal is told once per loop iteration, each only when it
//! changed: mouse capture, the colors, the title, the screen shown -- and
//! the panels' directories noted for the next start.

use std::io::Write;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use tracing::warn;

use crate::app::{App, Mode};


/// Mouse capture is on in the browser (a click on a panel's title edits
/// its path) and while an editor is open (F4, Compare or the conflict
/// resolver), synced once per loop iteration rather than at each place one
/// opens or closes. Capture takes over the terminal's own text selection
/// (Windows Terminal still selects with `Shift` held); the user screen has
/// its own (`user_screen/selection.rs`). Only set after the terminal call
/// succeeds -- `restore_terminal` relies on it.
pub(super) fn sync_mouse_capture(app: &mut App) {
    let wanted = matches!(app.mode, Mode::Browsing | Mode::Editing(_) | Mode::CompareFiles(_) | Mode::ResolveConflict(_));
    if wanted == app.mouse_capture_enabled {
        return;
    }
    let result = if wanted { execute!(std::io::stdout(), EnableMouseCapture) } else { execute!(std::io::stdout(), DisableMouseCapture) };
    match result {
        Ok(()) => app.mouse_capture_enabled = wanted,
        Err(err) => warn!(%err, wanted, "failed to toggle mouse capture"),
    }
}


/// The terminal's title (`OSC 2`): the active panel's directory, or the
/// file being edited -- what a tab is labelled with, in litastum's own
/// window and in Windows Terminal alike. Sent only when it changes.
pub(super) fn sync_title(app: &mut App) {
    let wanted = window_title(app);
    if app.terminal_title.as_deref() == Some(wanted.as_str()) {
        return;
    }
    if let Err(err) = execute!(std::io::stdout(), crossterm::terminal::SetTitle(&wanted)) {
        warn!(%err, "failed to set the terminal's title");
        return;
    }
    app.terminal_title = Some(wanted);
}


/// The name a tab shows: the edited file's or the directory's, the whole
/// path for a drive's root.
fn window_title(app: &App) -> String {
    let path = match &app.mode {
        Mode::Editing(editor) => editor.path().to_path_buf(),
        Mode::CompareFiles(_) => return "Compare".to_string(),
        Mode::ResolveConflict(_) => return "Conflict".to_string(),
        _ => app.panels[app.active].path.clone(),
    };
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}


/// Notes the panels' directories for the next start (`last_paths`): a
/// change is written 30 s later, from the background-task polling.
pub(super) fn sync_panel_paths(app: &mut App) {
    let current = [app.panels[0].path.clone(), app.panels[1].path.clone()];
    app.last_paths.note(current, std::time::Instant::now());
}


/// Tells the terminal which screen litastum is on (`screen_name`) as the
/// user variable `litastum_screen` (`user_var`). litastum's window keeps a
/// zoom per screen with it. Sent only when it changes.
pub(super) fn sync_screen(app: &mut App) {
    let wanted = screen_name(&app.mode);
    if app.terminal_screen == Some(wanted) {
        return;
    }
    if send(&crate::user_var::set_user_var("litastum_screen", wanted), "the screen") {
        app.terminal_screen = Some(wanted);
    }
}


/// The screen's name for its zoom (requested: the panels, the editor,
/// Compare and the resolver each keep their own). `main` is the panels --
/// an image preview included -- the name it had before the editor got
/// its own, so a saved zoom stays.
pub(super) fn screen_name(mode: &Mode) -> &'static str {
    match mode {
        Mode::Editing(_) => "editor",
        Mode::CompareFiles(_) => "compare",
        Mode::ResolveConflict(_) => "conflict",
        Mode::Browsing | Mode::ImagePreview(_) => "main",
    }
}


/// Hands the theme's colors to the terminal whenever they change -- on
/// the first frame and after F9 -> Color schemes -- rather than at each
/// place the theme is set. `restore_terminal` gives the terminal its own
/// back.
pub(super) fn sync_terminal_palette(app: &mut App) {
    let wanted = crate::terminal_palette::palette_sequence(&app.theme);
    if wanted == app.terminal_palette {
        return;
    }
    if wanted.as_deref().is_none_or(|sequence| send(sequence, "the colors")) {
        app.terminal_palette = wanted;
    }
}


/// Writes `sequence` to the terminal at once; `false`, logged, if it
/// couldn't -- the sync is tried again next iteration.
fn send(sequence: &str, what: &str) -> bool {
    let mut stdout = std::io::stdout();
    match stdout.write_all(sequence.as_bytes()).and_then(|()| stdout.flush()) {
        Ok(()) => true,
        Err(err) => {
            warn!(%err, "failed to tell the terminal {what}");
            false
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    /// Requested: the panels, the editor, Compare and the resolver each
    /// zoom on their own.
    #[test]
    fn the_editor_is_a_screen_of_its_own() {
        let dir = unique_scratch_dir("screen-name");
        let file = dir.join("file.txt");
        std::fs::write(&file, "text").unwrap();
        let editor = crate::editor::Editor::open(file, None, crate::editor::EditorKeymapMode::Standard).unwrap();

        assert_eq!(screen_name(&Mode::Browsing), "main");
        assert_eq!(screen_name(&Mode::Editing(editor)), "editor");
    }

    /// A tab is labelled with the active panel's directory.
    #[test]
    fn the_title_is_the_active_panels_directory() {
        let dir = unique_scratch_dir("title");
        let app = test_app(dir.clone());

        assert_eq!(window_title(&app), dir.file_name().unwrap().to_string_lossy());
    }
}
