use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::editor::{cancel_selection, text_key};
use crate::notice::Notice;
use crate::yes_no::{self, Answer};

use super::keys::{resolve, DiffCommand};
use super::menu::open_compare_menu;

/// Key handling for `Mode::CompareFiles` -- Compare-level commands
/// (above) are resolved first; anything left over that `edtui` itself
/// understands is forwarded straight into whichever pane currently has
/// focus (`CompareState::focused_mut`), exactly like plain typing
/// already reaches `Editor::input` from `editor::handle_editor_key`.
pub fn handle_compare_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Mode::CompareFiles(state) = &mut app.mode else {
        return Ok(Effect::None);
    };

    if state.path_edit.is_some() {
        if let Err(err) = state.path_edit_key(key) {
            app.notice = Some(Notice::error(err.to_string()));
        }
        return Ok(Effect::None);
    }

    let command = resolve(key);
    debug!(key = ?crate::logging::LoggedKey(key), ?command, "compare key");

    match command {
        DiffCommand::Close if cancel_selection(state.focused_mut(), key) => {}
        DiffCommand::Close => close_compare_or_confirm(app)?,
        DiffCommand::Save => {
            if let Err(err) = state.save_focused() {
                tracing::warn!(%err, "compare: failed to save the focused pane");
                app.notice = Some(Notice::error(format!("Save failed: {err}")));
            }
        }
        // Two panes: either way is the other one.
        DiffCommand::NextPane | DiffCommand::PreviousPane => state.toggle_focus(),
        DiffCommand::EditPath => state.start_path_edit(),
        DiffCommand::SaveAs => state.start_save_as(),
        DiffCommand::NextChange => state.jump_to_next_hunk(),
        DiffCommand::PreviousChange => state.jump_to_previous_hunk(),
        DiffCommand::OpenMenu => app.overlay = Some(Overlay::CompareMenu(open_compare_menu())),
        DiffCommand::Forward => text_key(state.focused_mut(), key),
        DiffCommand::Ignore => {}
    }

    Ok(Effect::None)
}

/// `Esc` on Compare: closes straight back to browsing if neither pane
/// has unsaved changes, otherwise opens `Overlay::ConfirmDiscard`
/// instead of discarding silently -- same shape as
/// `editor::close_editor_or_confirm`, just checking both of Compare's
/// panes (`CompareState::is_dirty`) instead of one editor.
fn close_compare_or_confirm(app: &mut App) -> Result<()> {
    let Mode::CompareFiles(state) = &app.mode else {
        return Ok(());
    };

    if !state.is_dirty() {
        app.mode = Mode::Browsing;
        return Ok(());
    }

    debug!("compare close: unsaved changes, asking to confirm discard");
    app.overlay = Some(Overlay::ConfirmDiscard);
    Ok(())
}

/// Key handling on the "discard unsaved changes?" prompt over Compare or
/// the conflict resolver -- either way, `Yes` drops back to browsing.
pub fn handle_compare_confirm_discard_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let answer = yes_no::answer(key);
    debug!(key = ?crate::logging::LoggedKey(key), ?answer, "compare confirm-discard key");

    match answer {
        Answer::Yes => {
            app.overlay = None;
            app.mode = Mode::Browsing;
        }
        Answer::No => app.overlay = None,
        Answer::Ignore => {}
    }

    Ok(Effect::None)
}


#[cfg(test)]
mod tests;
