use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, Mode, Overlay};

use crate::command_line::Effect;

use super::keys::key_effect;

/// Fires the fast paste once per physical `Ctrl+V` press (edge-triggered),
/// into any `paste_target` field -- each got the same ~7-8ms/char flood.
/// The editor also needs plain `Standard` typing (Vim's `Ctrl+V` is
/// visual block). `false`, with no swallow armed, when nothing can take a
/// paste. The flood handling itself is `windows_terminal::PasteFlood`'s.
/// History: docs/history/editor-performance.md.
#[cfg(windows)]
pub(super) fn try_intercept_paste_hotkey(app: &mut App) -> Result<bool> {
    if !app.paste_flood.ctrl_v_just_pressed() {
        return Ok(false);
    }

    let Some(target) = paste_target(app) else {
        return Ok(false);
    };
    if target == PasteTarget::EditorBuffer && !matches!(&app.mode, Mode::Editing(editor) if editor.is_plain_standard_typing()) {
        return Ok(false);
    }
    let text = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        Ok(text) => text,
        Err(_) => return Ok(false),
    };
    if text.is_empty() {
        return Ok(false);
    }

    let rest = app.paste_flood.not_yet_delivered(&text);
    if !rest.is_empty() {
        apply_paste(app, target, &rest)?;
        app.paste_flood.expect(&rest);
    }
    Ok(true)
}

/// Where a paste should land right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteTarget {
    /// The built-in editor's own buffer -- one fast splice,
    /// `Editor::paste_text`.
    EditorBuffer,
    /// A single-line text field (the command line, a search box, a
    /// prompt, ...) -- replayed as typed characters through the normal
    /// dispatch, with no redraw in between.
    TextField,
}

/// `None` for a mode with no text field at all, *or* one where a plain
/// character is a command rather than text -- `y`/`n` in a
/// confirmation, a drive letter in `Alt+F1`, `I`/`E` in the theme
/// picker, ... Replaying pasted text into one of those would silently
/// run whatever commands its characters happen to spell, which is why
/// this is an explicit allow-list, not "every mode but the editor's"
/// (what bracketed paste used to do before this existed).
fn paste_target(app: &App) -> Option<PasteTarget> {
    if let Some(overlay) = &app.overlay {
        return match overlay {
            Overlay::FindFile(state) if state.phase == crate::explorer::FindFilePhase::Typing => Some(PasteTarget::TextField),
            Overlay::CommandHistory(_)
            | Overlay::ConfirmTransfer(_)
            | Overlay::UserMenuPrompt(_)
            | Overlay::AddUserMenuItem(..)
            | Overlay::MarkdownLinkSearch(_) => Some(PasteTarget::TextField),
            _ => None,
        };
    }
    match &app.mode {
        // The embedded Markdown preview half of an editor+preview
        // session has no text field of its own.
        Mode::Editing(_) if app.markdown_edit_preview.is_some() && app.active == 1 => None,
        Mode::Editing(editor) if editor.is_searching() => Some(PasteTarget::TextField),
        Mode::Editing(_) if app.editor_save_as.is_some() => Some(PasteTarget::TextField),
        Mode::Editing(_) => Some(PasteTarget::EditorBuffer),
        Mode::Browsing => Some(PasteTarget::TextField),
        Mode::CompareFiles(state) if state.path_edit.is_some() => Some(PasteTarget::TextField),
        Mode::ResolveConflict(state) if state.is_editing_path() => Some(PasteTarget::TextField),
        _ => None,
    }
}

/// Applies `text` to `target` -- see `PasteTarget`'s own variants. A
/// line break is skipped rather than replayed as `Enter` for a text
/// field: these fields are single-line, and forwarding it could submit
/// a command/form the user never meant to trigger this instant, the
/// same "may execute unexpected commands" concern Windows Terminal's
/// own multi-line-paste warning is about.
fn apply_paste(app: &mut App, target: PasteTarget, text: &str) -> Result<()> {
    match target {
        PasteTarget::EditorBuffer => {
            if let Mode::Editing(editor) = &mut app.mode {
                editor.paste_text(text);
            }
        }
        PasteTarget::TextField => {
            for ch in text.chars().filter(|&ch| ch != '\n' && ch != '\r') {
                // A plain typed character never asks for terminal work.
                let effect = key_effect(app, crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))?;
                debug_assert_eq!(effect, Effect::None);
            }
        }
    }
    Ok(())
}

/// A bracketed paste (Unix only in practice -- `crossterm`'s Windows
/// backend never produces `Event::Paste`), routed like the Windows
/// `Ctrl+V` bypass. That shared routing fixed text replayed as keystrokes
/// into modes where a letter is a command, and a paste going into the file
/// while the `Ctrl+F` box was open.
pub(super) fn handle_paste_event(app: &mut App, text: &str) -> Result<()> {
    match paste_target(app) {
        Some(target) => apply_paste(app, target, text),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{editing_app, test_app, unique_scratch_dir};

    fn editor_app(contents: &str) -> App {
        editing_app(contents, EditorKeymapMode::Standard)
    }

    #[test]
    fn text_fields_and_the_editor_buffer_are_paste_targets() {
        let mut app = test_app(unique_scratch_dir("paste-routing"));
        assert_eq!(paste_target(&app), Some(PasteTarget::TextField), "the always-live command line");

        app.overlay = Some(Overlay::FindFile(crate::explorer::FindFileState::new()));
        assert_eq!(paste_target(&app), Some(PasteTarget::TextField), "Find file's own fields while typing");

        let mut results = crate::explorer::FindFileState::new();
        results.phase = crate::explorer::FindFilePhase::Results;
        app.overlay = Some(Overlay::FindFile(results));
        assert_eq!(paste_target(&app), None, "the results list has no text field");

        let app = editor_app("hello");
        assert_eq!(paste_target(&app), Some(PasteTarget::EditorBuffer));
    }

    /// A mode where a plain character is a command must never get
    /// pasted text replayed into it -- bracketed paste used to do
    /// exactly that for every non-editor mode.
    #[test]
    fn a_mode_without_a_text_field_ignores_a_paste() {
        let mut app = test_app(unique_scratch_dir("paste-routing"));
        app.overlay = Some(Overlay::Info("read me".to_string()));

        handle_paste_event(&mut app, "anything").unwrap();

        assert!(matches!(app.overlay, Some(Overlay::Info(_))), "any key dismisses Info -- the paste must not have been replayed as keys");
    }

    #[test]
    fn pasting_into_the_command_line_drops_line_breaks() {
        let mut app = test_app(unique_scratch_dir("paste-routing"));

        handle_paste_event(&mut app, "svn st\r\n--quiet").unwrap();

        assert_eq!(app.command_line.text(), "svn st--quiet");
    }

    /// With a prompt or menu open over the editor, a paste must not
    /// reach the buffer underneath.
    #[test]
    fn a_paste_over_an_overlay_leaves_the_editor_alone() {
        let mut app = editor_app("hello
");
        app.overlay = Some(crate::app::Overlay::ConfirmDiscard);

        handle_paste_event(&mut app, "pasted").unwrap();

        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert!(!editor.is_dirty());
    }

    /// Real bug, found while routing both paste paths through
    /// `paste_target`: with the `Ctrl+F` box open, a paste went into
    /// the file's own buffer, not the box.
    #[test]
    fn pasting_while_the_editor_search_box_is_open_fills_the_box_not_the_buffer() {
        let mut app = editor_app("hello world\n");
        let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
        editor.start_search();

        handle_paste_event(&mut app, "world").unwrap();

        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert_eq!(editor.search_query(), "world");
        assert!(!editor.is_dirty(), "the buffer itself must be untouched");
    }
}
