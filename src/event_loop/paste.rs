use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::{App, Mode};

use super::keys::dispatch_key_event;

/// Edge-triggered: fires the fast paste path once per distinct real
/// physical `Ctrl+V` press (`PasteFlood::ctrl_v_just_pressed`), never
/// repeatedly while the combo stays held. Returns `true` (asking
/// `wait_for_event` to return and let `run()` redraw) only when
/// something actually happened -- a fresh press while nothing can take a
/// paste (`paste_target` is `None`) is left completely alone, no swallow
/// armed, so `crossterm`'s normal event flow handles it exactly as
/// before.
///
/// Covers every text field, not just the editor's own buffer --
/// reported directly that pasting into a search field (Find file, the
/// editor's own `Ctrl+F` box, ...) was slow too. It was the same cause
/// as the editor's own paste had been: Windows Terminal owns `Ctrl+V`
/// and feeds the clipboard in as simulated keystrokes at ~7-8ms each,
/// so even a field whose own per-key handling costs a fraction of a
/// millisecond (measured: the command line and Find file, key plus
/// redraw, ~0.25ms) couldn't paste faster than that. The editor's own
/// buffer additionally requires plain `Standard` typing
/// (`Editor::is_plain_standard_typing`) -- Vim's own `Ctrl+V` means
/// visual-block mode, not paste.
///
/// See `windows_terminal`'s own module doc for why a real terminal
/// `Ctrl+V` needs this bypass in the first place. Everything about the
/// keystroke flood that follows it -- the edge-triggered press, pasting
/// only what the flood hasn't already delivered, and swallowing the rest
/// of it once it arrives -- is `windows_terminal::PasteFlood`'s; this
/// only decides where the paste goes (`paste_target`) and applies it.
#[cfg(windows)]
pub(super) fn try_intercept_paste_hotkey(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<bool> {
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
        apply_paste(app, terminal, target, &rest)?;
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
    match &app.mode {
        // The embedded Markdown preview half of an editor+preview
        // session has no text field of its own.
        Mode::Editing(_) if app.markdown_edit_preview.is_some() && app.active == 1 => None,
        Mode::Editing(editor) if editor.is_searching() => Some(PasteTarget::TextField),
        Mode::Editing(_) => Some(PasteTarget::EditorBuffer),
        Mode::FindFile(state) if state.phase == crate::explorer::FindFilePhase::Typing => Some(PasteTarget::TextField),
        Mode::Browsing
        | Mode::CommandHistory(_)
        | Mode::ConfirmTransfer(_)
        | Mode::UserMenuPrompt(_)
        | Mode::AddUserMenuItem(..)
        | Mode::MarkdownLinkSearch(..) => Some(PasteTarget::TextField),
        _ => None,
    }
}

/// Applies `text` to `target` -- see `PasteTarget`'s own variants. A
/// line break is skipped rather than replayed as `Enter` for a text
/// field: these fields are single-line, and forwarding it could submit
/// a command/form the user never meant to trigger this instant, the
/// same "may execute unexpected commands" concern Windows Terminal's
/// own multi-line-paste warning is about.
fn apply_paste(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, target: PasteTarget, text: &str) -> Result<()> {
    match target {
        PasteTarget::EditorBuffer => {
            if let Mode::Editing(editor) = &mut app.mode {
                editor.paste_text(text);
            }
        }
        PasteTarget::TextField => {
            for ch in text.chars().filter(|&ch| ch != '\n' && ch != '\r') {
                dispatch_key_event(app, crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), terminal)?;
            }
        }
    }
    Ok(())
}

/// A bracketed paste (`EnableBracketedPaste` in `setup_terminal`) --
/// see its own doc comment for the real reported bug this exists to
/// fix. Unix-only in practice (`crossterm`'s Windows backend never
/// produces `Event::Paste`, see `windows_terminal`). Goes through the
/// same `paste_target`/`apply_paste` as the Windows `Ctrl+V` bypass --
/// the terminal already handed the text over, so there's no clipboard
/// read, and no keystroke flood to swallow afterward either.
///
/// Two real problems this fixed by sharing that routing: every mode
/// that wasn't the editor used to get the text replayed as keystrokes
/// unconditionally (including ones where a letter is a command), and
/// an open `Ctrl+F` box still sent the paste into the file's own
/// buffer instead of the box.
pub(super) fn handle_paste_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, text: &str) -> Result<()> {
    match paste_target(app) {
        Some(target) => apply_paste(app, terminal, target, text),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Editor, EditorKeymapMode};
    use crate::test_support::{test_app, unique_scratch_dir};

    fn dummy_terminal() -> Terminal<CrosstermBackend<Stdout>> {
        Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap()
    }

    fn editor_app(contents: &str) -> App {
        let dir = unique_scratch_dir("paste-routing");
        let path = dir.join("file.txt");
        std::fs::write(&path, contents).unwrap();
        let mut app = test_app(dir);
        app.mode = Mode::Editing(Editor::open(path, None, EditorKeymapMode::Standard).unwrap());
        app
    }

    #[test]
    fn text_fields_and_the_editor_buffer_are_paste_targets() {
        let mut app = test_app(unique_scratch_dir("paste-routing"));
        assert_eq!(paste_target(&app), Some(PasteTarget::TextField), "the always-live command line");

        app.mode = Mode::FindFile(crate::explorer::FindFileState::new());
        assert_eq!(paste_target(&app), Some(PasteTarget::TextField), "Find file's own fields while typing");

        let mut results = crate::explorer::FindFileState::new();
        results.phase = crate::explorer::FindFilePhase::Results;
        app.mode = Mode::FindFile(results);
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
        app.mode = Mode::Info("read me".to_string());

        handle_paste_event(&mut app, &mut dummy_terminal(), "anything").unwrap();

        assert!(matches!(app.mode, Mode::Info(_)), "any key dismisses Info -- the paste must not have been replayed as keys");
    }

    #[test]
    fn pasting_into_the_command_line_drops_line_breaks() {
        let mut app = test_app(unique_scratch_dir("paste-routing"));

        handle_paste_event(&mut app, &mut dummy_terminal(), "svn st\r\n--quiet").unwrap();

        assert_eq!(app.command_line, "svn st--quiet");
    }

    /// Real bug, found while routing both paste paths through
    /// `paste_target`: with the `Ctrl+F` box open, a paste went into
    /// the file's own buffer, not the box.
    #[test]
    fn pasting_while_the_editor_search_box_is_open_fills_the_box_not_the_buffer() {
        let mut app = editor_app("hello world\n");
        let Mode::Editing(editor) = &mut app.mode else { unreachable!() };
        editor.start_search();

        handle_paste_event(&mut app, &mut dummy_terminal(), "world").unwrap();

        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert_eq!(editor.search_query(), "world");
        assert!(!editor.is_dirty(), "the buffer itself must be untouched");
    }
}
