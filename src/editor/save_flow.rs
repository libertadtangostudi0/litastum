//! `Ctrl+S`/`F2` in any editor -- F4's, a Compare or resolver pane: the
//! focused one is saved, unless its file changed on disk since it was read
//! or last saved; then `Overlay::ConfirmOverwrite` asks first (requested:
//! a change made meanwhile -- another program, `hg pull` -- was written
//! over unseen). History: docs/history/editor-saving.md.

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::warn;

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::notice::Notice;
use crate::yes_no::{self, Answer};

use super::editor::Saved;
use super::Editor;


/// Saves the focused editor, or asks if its file changed on disk.
pub fn save_focused_or_ask(app: &mut App) {
    let Some(editor) = focused_editor(app) else {
        return;
    };
    let outcome = editor.save_checked();
    let path = editor.path().to_path_buf();
    match outcome {
        Ok(Saved::Done) => saved(app),
        Ok(Saved::ChangedOnDisk) => app.overlay = Some(Overlay::ConfirmOverwrite(path)),
        Err(err) => failed(app, &err),
    }
}


/// The question's keys: `Y` writes over the file, `N`/`Esc` go back to
/// editing with nothing written.
pub fn handle_confirm_overwrite_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    match yes_no::answer(key) {
        Answer::Yes => {
            app.overlay = None;
            let result = focused_editor(app).map(Editor::save);
            match result {
                Some(Ok(())) => saved(app),
                Some(Err(err)) => failed(app, &err),
                None => {}
            }
        }
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }
    Ok(Effect::None)
}


fn focused_editor(app: &mut App) -> Option<&mut Editor> {
    match &mut app.mode {
        Mode::Editing(editor) => Some(editor),
        Mode::CompareFiles(state) => Some(state.focused_mut()),
        Mode::ResolveConflict(state) => Some(state.focused_mut()),
        _ => None,
    }
}


/// After a save: a linked Markdown preview shows the saved text.
fn saved(app: &mut App) {
    if matches!(app.mode, Mode::Editing(_)) {
        if let Some(preview) = &mut app.markdown_edit_preview {
            preview.reload();
        }
    }
}


/// A failed save is shown, not returned: an `Err` from a key handler ends
/// the event loop.
fn failed(app: &mut App, err: &std::io::Error) {
    warn!(%err, "save failed");
    app.notice = Some(Notice::error(format!("Save failed: {err}")));
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{editing_app, key};

    fn edited_app() -> (App, std::path::PathBuf) {
        let mut app = editing_app("mine\n", EditorKeymapMode::Standard);
        let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
        editor.input(key(KeyCode::Char('!')));
        let path = editor.path().to_path_buf();
        (app, path)
    }

    /// Change the file's length too: a time stamp alone could be the
    /// same within the file system's resolution.
    fn change_on_disk(path: &std::path::Path) {
        std::fs::write(path, "someone else's, longer\n").unwrap();
    }

    #[test]
    fn an_unchanged_file_is_just_saved() {
        let (mut app, path) = edited_app();

        save_focused_or_ask(&mut app);

        assert!(app.overlay.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!mine\n");
    }

    /// Requested: a change made on disk meanwhile isn't written over
    /// unseen -- the save asks, `Esc` keeps it, `Y` writes over it.
    #[test]
    fn a_file_changed_on_disk_asks_before_it_is_written_over() {
        let (mut app, path) = edited_app();
        change_on_disk(&path);

        save_focused_or_ask(&mut app);
        assert!(matches!(&app.overlay, Some(Overlay::ConfirmOverwrite(asked)) if *asked == path));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "someone else's, longer\n", "nothing written yet");

        handle_confirm_overwrite_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(app.overlay.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "someone else's, longer\n", "kept");

        save_focused_or_ask(&mut app);
        handle_confirm_overwrite_key(&mut app, key(KeyCode::Char('y'))).unwrap();
        assert!(app.overlay.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!mine\n", "written over");

        save_focused_or_ask(&mut app);
        assert!(app.overlay.is_none(), "after a save, the file is as saved: no question");
    }

    /// A file deleted meanwhile asks too: saving would make it again.
    #[test]
    fn a_file_deleted_on_disk_asks_too() {
        let (mut app, path) = edited_app();
        std::fs::remove_file(&path).unwrap();

        save_focused_or_ask(&mut app);

        assert!(matches!(app.overlay, Some(Overlay::ConfirmOverwrite(_))));
    }
}
