use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::{App, Mode, Overlay};
use crate::command_line::Effect;
use crate::{command_line, compare, editor, explorer, keyboard_layout, theming};

use super::drain_pending_mouse_events;

/// `Up`/`Down`/`PageUp`/`PageDown`, held down, generate a rapid burst of
/// distinct `KeyEventKind::Press` events via the OS's own key-repeat --
/// `crossterm`'s Windows backend never reports a separate "repeat" kind
/// distinguishing these from a fresh press (unlike some Unix terminals),
/// so each one looks like an ordinary keystroke and gets its own full
/// dispatch. These four are the ones actually meant to be held for a
/// stretch to move through a long list/document/buffer, the exact same
/// shape of problem `drain_pending_mouse_events` already fixed for a
/// touchpad's own scroll-wheel burst -- reported directly against the
/// built-in editor's own text caret specifically (it kept traveling
/// past where the user stopped scrolling, and reversing direction took
/// too long to catch up), but the same backlog can
/// build up navigating a long file panel listing or popup list too,
/// since `dispatch_key_event`'s own big match is one shared chokepoint
/// for all of them.
fn is_repeatable_navigation_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown)
}

/// Whether `next` reverses the axis `prev` was moving on -- `Up`/`Down`
/// undo each other, `PageUp`/`PageDown` undo each other, but a
/// same-key repeat or a switch between the two axes isn't a reversal at
/// all (there's no "held down, then immediately Page Up" gesture this
/// needs to special-case the way a scroll wheel's own two-directions-only
/// axis does).
fn is_navigation_reversal(prev: KeyCode, next: KeyCode) -> bool {
    matches!((prev, next), (KeyCode::Up, KeyCode::Down) | (KeyCode::Down, KeyCode::Up) | (KeyCode::PageUp, KeyCode::PageDown) | (KeyCode::PageDown, KeyCode::PageUp))
}

/// Dispatches `key`, then -- only when it was one of the four
/// held-to-scroll keys above -- drains any further same-axis repeats
/// already queued up, redrawing once for the whole burst instead of
/// once per repeat, exactly mirroring `drain_pending_mouse_events`'s
/// own reasoning and its immediate-stop-on-reversal behavior. Every
/// other key (typing, `Left`/`Right`, `Enter`, `Esc`, ...) is completely
/// unaffected -- dispatched once, same as always, since those don't
/// have this repeat-burst shape (`Left`/`Right` move by a single
/// character/column, not a whole page, so a repeat backlog there is
/// nowhere near long enough to notice, and coalescing typed characters
/// would be actively wrong).
///
/// Returns whether anything was actually dispatched -- `false` only for
/// a bare `key.kind != KeyEventKind::Press` (`dispatch_key_event`'s own
/// very first check no-ops on those unconditionally, for every mode).
/// Windows' Console API reports a real key-up `KeyEventKind::Release`
/// for every physical keypress, not just the down-stroke
/// (`is_repeatable_navigation_key`'s own doc comment already covers the
/// down-stroke's own repeat behavior) -- before this, `run()`'s own loop
/// redrew once for *that* too, a real, reported symptom: the terminal's
/// own real cursor briefly visits wherever `Release` leaves it during
/// that spurious redraw (a screen redraw always ends by repositioning
/// the terminal cursor for whatever `Mode` is now active) before the
/// state genuinely settles, reading as a visible flicker/jump on
/// certain terminals for keys whose own redraw is otherwise cheap
/// enough to land inside one visible refresh window (`Delete`/`Backspace`
/// on the command line, reported directly). Skipping the redraw
/// entirely for a key `dispatch_key_event` never touched removes that
/// spurious frame outright, for every key, not just the one reported.
pub(super) fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<bool> {
    // Normalized first so a `Ctrl+V` typed under a non-Latin layout is
    // still recognized by the swallow's own double-paste guard.
    let normalized = keyboard_layout::normalize_ctrl_shortcut(key);
    if key.kind == KeyEventKind::Press && app.paste_flood.should_swallow(normalized.code, normalized.modifiers) {
        return Ok(false);
    }
    if key.kind == KeyEventKind::Press {
        app.paste_flood.record_typed_key(normalized.code, normalized.modifiers);
    }

    dispatch_key_event(app, key, terminal)?;
    if key.kind == KeyEventKind::Press {
        if is_repeatable_navigation_key(key.code) {
            drain_pending_navigation_keys(app, terminal, key.code)?;
        } else if is_plain_typed_char(key.code, key.modifiers) && editor_accepting_plain_typing(app) {
            drain_pending_editor_typing(app, terminal)?;
        }
    }
    Ok(key.kind == KeyEventKind::Press)
}

/// A `Char` key with no `Ctrl`/`Alt` held -- ordinary typed text
/// (`Shift` included, for an uppercase letter), never a shortcut.
/// `drain_pending_editor_typing`'s own gate for what's safe to fold
/// into one batched insert.
fn is_plain_typed_char(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::Char(_)) && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Whether the built-in editor is the thing that would actually receive
/// a plain typed character right now -- mirrors `dispatch_key_event`'s
/// own `Mode::Editing` guard exactly, including the embedded-Markdown-
/// preview carve-out (while the *preview* half has focus, plain
/// characters go to `explorer::handle_markdown_edit_preview_key`
/// instead, never the editor), plus `Editor::is_plain_standard_typing`'s
/// own keymap/mode check.
fn editor_accepting_plain_typing(app: &App) -> bool {
    if app.overlay.is_some() {
        return false;
    }
    let Mode::Editing(editor) = &app.mode else {
        return false;
    };
    if app.markdown_edit_preview.is_some() && app.active == 1 {
        return false;
    }
    editor.is_plain_standard_typing()
}

/// Applies a burst of already-queued plain characters to the editor in
/// one go: on Windows a terminal paste (and fast typing) arrives as
/// ordinary key events with no `Event::Paste`, and `run()` would
/// otherwise redraw once per character. History:
/// docs/history/editor-performance.md (Round 2).
///
/// Mirrors `drain_pending_navigation_keys`'s own "coalesce a burst,
/// redraw once" shape: drains every already-queued plain character key
/// (`is_plain_typed_char`) for as long as the editor would still accept
/// one as plain typing (`editor_accepting_plain_typing` -- re-checked on
/// every iteration, not just once, in case some other queued event
/// changes mode mid-burst), and applies the whole batch in one
/// `Editor::paste_text` call -- the same fast, from-scratch splice
/// `Ctrl+V`/a real bracketed paste already use
/// (`editor::fast_paste::splice_paste`), rather than one `InsertChar` per character. A
/// key found mid-burst that isn't a plain character (or that arrives
/// once the editor would no longer treat one as plain typing) is never
/// silently dropped, same rule every sibling drain function in this
/// file already follows -- the batch collected so far is flushed first,
/// then that key/event is dispatched normally before this returns.
fn drain_pending_editor_typing(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut batch = String::new();
    while event::poll(std::time::Duration::from_secs(0))? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press && is_plain_typed_char(key.code, key.modifiers) && editor_accepting_plain_typing(app) => {
                if let KeyCode::Char(c) = key.code {
                    batch.push(c);
                }
                app.paste_flood.record_typed_key(key.code, key.modifiers);
            }
            Event::Key(key) => {
                flush_editor_typing_batch(app, &mut batch);
                return dispatch_key_event(app, key, terminal);
            }
            Event::Mouse(mouse) => {
                flush_editor_typing_batch(app, &mut batch);
                super::handle_mouse(app, mouse);
                let last_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown).then_some(mouse.kind);
                return drain_pending_mouse_events(app, terminal, last_scroll);
            }
            _ => {}
        }
    }
    flush_editor_typing_batch(app, &mut batch);
    Ok(())
}

/// Applies `batch` (if non-empty) to the editor via `Editor::paste_text`
/// and clears it -- shared by every early-return branch in
/// `drain_pending_editor_typing` above, so a batch collected so far is
/// never silently lost just because the drain is about to end for a
/// different reason.
fn flush_editor_typing_batch(app: &mut App, batch: &mut String) {
    if batch.is_empty() {
        return;
    }
    if let Mode::Editing(editor) = &mut app.mode {
        editor.paste_text(batch);
    }
    batch.clear();
}

/// See `handle_key_event`'s own doc comment. A key found mid-burst that
/// isn't one of the four repeatable navigation keys (or a mouse event)
/// is still fully dispatched -- it just ends this drain, same "never
/// silently drop an event, just stop coalescing" rule
/// `drain_pending_mouse_events` already follows.
fn drain_pending_navigation_keys(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, mut last: KeyCode) -> Result<()> {
    while event::poll(std::time::Duration::from_secs(0))? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press && is_repeatable_navigation_key(key.code) => {
                let reversed = is_navigation_reversal(last, key.code);
                dispatch_key_event(app, key, terminal)?;
                if reversed {
                    return Ok(());
                }
                last = key.code;
            }
            Event::Key(key) => return dispatch_key_event(app, key, terminal),
            Event::Mouse(mouse) => {
                super::handle_mouse(app, mouse);
                let last_scroll = matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown).then_some(mouse.kind);
                return drain_pending_mouse_events(app, terminal, last_scroll);
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn dispatch_key_event(app: &mut App, key: crossterm::event::KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let effect = key_effect(app, key)?;
    command_line::apply_effect(app, terminal, effect)
}


/// Routes one key to the current mode's handler and returns whatever
/// terminal work it asked for -- no `Terminal` here, so the whole
/// routing is testable.
pub(super) fn key_effect(app: &mut App, key: crossterm::event::KeyEvent) -> Result<Effect> {
    if key.kind != KeyEventKind::Press {
        return Ok(Effect::None);
    }

    // See `keyboard_layout::normalize_ctrl_shortcut`'s own doc comment
    // -- a `Ctrl+<letter>` chord typed under a non-Latin layout (a real
    // report: `Ctrl+C` in the editor did nothing at all under a Russian
    // layout) arrives with a layout-translated `char`, not the Latin
    // one every binding in this app is written against. Normalized once
    // here, ahead of every mode's own key handling, rather than in each
    // of them separately.
    let key = keyboard_layout::normalize_ctrl_shortcut(key);

    // Drives the alternate F-key row (`ui::draw_function_keys`). On
    // Windows this is mostly redundant with `wait_for_event`'s own
    // `GetAsyncKeyState` poll (which already catches real hold/release
    // even between keystrokes) but harmless to also set here; on other
    // platforms this keystroke-modifier reading is the only signal
    // there is -- see `App::alt_held`'s doc.
    app.alt_held = key.modifiers.contains(KeyModifiers::ALT);

    // An open overlay gets every key; the screen underneath gets none.
    // Most handlers need nothing from the terminal and return `()`; the
    // ones that can (history, the F2 menu, the browser) return their
    // `Effect` directly.
    if let Some(overlay) = &app.overlay {
        let handled = match overlay {
            Overlay::ConfirmDiscard if matches!(app.mode, Mode::CompareFiles(_)) => compare::handle_compare_confirm_discard_key(app, key),
            Overlay::ConfirmDiscard => editor::handle_confirm_discard_key(app, key),
            Overlay::EditorMenu(_) => editor::handle_editor_menu_key(app, key),
            Overlay::EditorKeymapMenu(_) => editor::handle_editor_keymap_menu_key(app, key),
            Overlay::CompareMenu(_) => compare::handle_compare_menu_key(app, key),
            Overlay::CompareLineEndingMenu(_) => compare::handle_compare_line_ending_menu_key(app, key),
            Overlay::ConfirmDelete(_) => explorer::handle_confirm_delete_key(app, key),
            Overlay::ConfirmTransfer(_) => explorer::handle_confirm_transfer_key(app, key),
            Overlay::MainMenu(_) => theming::handle_main_menu_key(app, key),
            Overlay::ThemeMenu(_) => theming::handle_theme_menu_key(app, key),
            Overlay::ShellMenu(_) => command_line::handle_shell_menu_key(app, key),
            Overlay::PopupStyleMenu(_) => theming::handle_popup_style_menu_key(app, key),
            Overlay::FindFile(_) => explorer::handle_find_file_key(app, key),
            Overlay::CommandHistory(_) => return command_line::handle_history_key(app, key),
            Overlay::ChangeDrive(_) => explorer::handle_drive_menu_key(app, key),
            Overlay::UserMenu(_) => return explorer::handle_user_menu_key(app, key),
            Overlay::UserMenuPrompt(_) => return explorer::handle_user_menu_prompt_key(app, key),
            Overlay::ConfirmPortFarMenu(_) => explorer::handle_confirm_port_far_menu_key(app, key),
            Overlay::AddUserMenuItem(..) => explorer::handle_add_user_menu_item_key(app, key),
            Overlay::MarkdownLinkSearch(_) => {
                explorer::handle_markdown_link_search_key(app, key);
                Ok(())
            }
            // Any key dismisses -- there's nothing to answer, just
            // something to acknowledge having read.
            Overlay::Info(_) => {
                app.overlay = None;
                Ok(())
            }
        };
        return handled.map(|()| Effect::None);
    }

    let handled = match &app.mode {
        // `Tab` toggles which half of a combined editor+preview session
        // (`App::markdown_edit_preview`) keyboard input reaches -- `0`
        // the editor, `1` the embedded preview -- intercepted here,
        // ahead of both `editor::handle_editor_key` and
        // `explorer::handle_markdown_edit_preview_key`, since it's
        // meaningless to either on its own (plain `F4` editing, with no
        // linked preview, forwards `Tab` straight to the editor as
        // always -- see the `_ if` guard's own condition).
        Mode::Editing(_) if app.markdown_edit_preview.is_some() && key.code == KeyCode::Tab => {
            app.active = 1 - app.active;
            Ok(())
        }
        Mode::Editing(_) if app.markdown_edit_preview.is_some() && app.active == 1 => explorer::handle_markdown_edit_preview_key(app, key),
        Mode::Editing(_) => editor::handle_editor_key(app, key),
        Mode::CompareFiles(_) => compare::handle_compare_key(app, key),
        Mode::ImagePreview(_) => {
            explorer::handle_image_preview_key(app, key);
            Ok(())
        }
        Mode::Browsing => return command_line::handle_browsing_key(app, key),
    };
    handled.map(|()| Effect::None)
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::app::Overlay;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{editing_app, key};

    fn editor_app() -> App {
        editing_app("hello
", EditorKeymapMode::Standard)
    }

    #[test]
    fn an_open_overlay_gets_the_keys_instead_of_the_editor_underneath() {
        let mut app = editor_app();
        app.overlay = Some(Overlay::ConfirmDiscard);

        key_effect(&mut app, key(KeyCode::Char('x'))).unwrap();

        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert!(!editor.is_dirty(), "the typed key must not reach the editor");
        assert!(matches!(app.overlay, Some(Overlay::ConfirmDiscard)), "an unrelated key leaves the prompt open");
    }

    #[test]
    fn closing_the_overlay_returns_keys_to_the_editor() {
        let mut app = editor_app();
        app.overlay = Some(Overlay::ConfirmDiscard);

        key_effect(&mut app, key(KeyCode::Esc)).unwrap();
        key_effect(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(app.overlay.is_none());
        let Mode::Editing(editor) = &app.mode else { unreachable!() };
        assert!(editor.is_dirty());
    }

    #[test]
    fn f9_esc_round_trip_keeps_the_same_editor_open() {
        let mut app = editor_app();
        key_effect(&mut app, key(KeyCode::Char('x'))).unwrap();

        key_effect(&mut app, key(KeyCode::F(9))).unwrap();
        assert!(matches!(app.overlay, Some(Overlay::EditorMenu(_))));
        key_effect(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none());
        let Mode::Editing(editor) = &app.mode else { panic!("expected the editor to still be open") };
        assert!(editor.is_dirty(), "the unsaved edit survives opening and closing the menu");
    }
}
