use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::{App, Mode};

use super::keys::dispatch_key_event;

/// Edge-triggered: fires the fast paste path once per distinct real
/// physical `Ctrl+V` press (`app.ctrl_v_physically_held` tracks the
/// previous poll's state, same pattern `wait_for_event`'s own
/// `alt_held` tracking already uses), never repeatedly while the combo
/// stays held. Returns `true` (asking `wait_for_event` to return and
/// let `run()` redraw) only when something actually happened -- a fresh
/// press while nothing can take a paste (`paste_target` is `None`) is
/// left completely alone, no swallow armed, so `crossterm`'s normal
/// event flow handles it exactly as before.
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
/// See `paste_hotkey.rs`'s own module doc comment for why a real
/// terminal `Ctrl+V` needs this bypass in the first place, and for how
/// the mismatch between "read the clipboard right now" and "Windows
/// Terminal's own flood is still coming, one keystroke at a time" is
/// resolved: `app.pending_paste_swallow` is armed here (to the pasted
/// text's own keystroke-equivalent length) so `handle_key_event` can
/// silently discard that flood once it actually arrives, instead of
/// typing the same text a second time right after this already pasted
/// it once, instantly. For a single-line field that also keeps the
/// flood's own `Enter` keystrokes (one per pasted line break) from
/// submitting the field.
#[cfg(windows)]
pub(super) fn try_intercept_paste_hotkey(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<bool> {
    let held = crate::paste_hotkey::ctrl_v_physically_down();
    let just_pressed = held && !app.ctrl_v_physically_held;
    app.ctrl_v_physically_held = held;
    if !just_pressed {
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

    apply_paste(app, terminal, target, &text)?;
    arm_paste_swallow(app, &text);
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

/// Sets up `app.pending_paste_swallow`/`_deadline` right after a fast
/// paste (`try_intercept_paste_hotkey`) so `handle_key_event` knows what
/// Windows Terminal's own still-incoming keystroke flood is expected to
/// look like, in order, and can silently discard it -- see
/// `App::pending_paste_swallow`'s own doc comment for the queue itself,
/// and `should_swallow_paste_tail`'s for why content-matching (not just
/// counting) is what actually fixes real typing getting stuck behind a
/// still-draining swallow window.
///
/// **Extends the existing queue, never overwrites it.** Real reported
/// bug: pasting again quickly (before the *first* paste's own flood had
/// finished arriving) left a visible, slow, character-by-character
/// typing delay afterward -- overwriting `pending_paste_swallow` with
/// just the second paste's own text threw away whatever was left of
/// the first paste's still-incoming flood, so those leftover characters
/// no longer matched anything expected and got typed as real (if
/// nonsensical) input, one throttled keystroke at a time, right where
/// `should_swallow_paste_tail`'s own "an expired deadline or a mismatch
/// ends the swallow" rule tries to protect *genuine* typing.
/// Appending instead means a still-pending tail from an earlier paste
/// keeps getting silently discarded first, in the same order the two
/// floods should actually arrive in (Windows Terminal processes one
/// paste's own injection before starting the next), with the new
/// paste's own expected characters queued up right after it.
///
/// `\r`-stripped -- a `\n` in the pasted text becomes one `Enter`
/// keystroke, everything else becomes one `Char` keystroke, but a `\r`
/// immediately before a `\n` (Windows-style line endings) never becomes
/// a keystroke of its own at all, matching
/// `editor::fast_paste::splice_paste`'s own normalization.
#[cfg(windows)]
fn arm_paste_swallow(app: &mut App, text: &str) {
    app.pending_paste_swallow.extend(text.chars().filter(|&c| c != '\r'));
    // Generous relative to the ~7-8ms/char rate this was actually
    // measured at (`logs/litastum.log`, see `paste_hotkey.rs`'s own
    // doc comment) -- this only exists to eventually give up if the
    // flood never arrives, not to race it. Recomputed from the whole
    // (possibly just-extended) queue, not just this call's own text, so
    // a second paste's own budget still covers whatever's left of an
    // earlier one queued ahead of it.
    let budget_ms = (app.pending_paste_swallow.len() as u64).saturating_mul(100).max(2000);
    app.pending_paste_swallow_deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(budget_ms));
}

/// A bracketed paste (`EnableBracketedPaste` in `setup_terminal`) --
/// see its own doc comment for the real reported bug this exists to
/// fix. Unix-only in practice (`crossterm`'s Windows backend never
/// produces `Event::Paste`, see `paste_hotkey.rs`). Goes through the
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

/// See `App::pending_paste_swallow`'s own doc comment -- `true` means
/// "this key matches the next expected character of Windows Terminal's
/// own still-incoming paste flood, already applied instantly by
/// `try_intercept_paste_hotkey`; discard it rather than typing the same
/// text a second time." Pops the queue's front on every swallow, and
/// clears it entirely (without swallowing this particular key) once
/// either it drains naturally, `pending_paste_swallow_deadline` has
/// passed, or this key simply doesn't match what was expected next.
///
/// **Matches by content, not just by shape** -- reported directly:
/// pressing `Enter` several times right after a paste only registered
/// with a 5-10 second delay. An earlier version of this only checked
/// whether a key was *character-shaped* (a plain `Char` or bare
/// `Enter`, no `Ctrl`/`Alt`) before swallowing it, with a plain
/// decrementing counter -- which also matches perfectly ordinary
/// keystrokes the user types *during* the still-draining swallow
/// window (a real `Enter` looks identical in shape to a flood `Enter`),
/// so genuine typing got silently eaten and had to wait for the whole
/// window to finish before anything else could get through. Comparing
/// against the *actual* pasted text's own next character instead means
/// a real keystroke that doesn't happen to match what the flood would
/// send next (the overwhelming majority of the time) is recognized
/// immediately and handled right away, not swallowed.
///
/// The same real report from before this fix still applies to *why*
/// modifiers matter: `Ctrl+S`/`Ctrl+Z` must never match regardless of
/// their `Char` code, since Windows Terminal's own flood only ever
/// injects the pasted text's own literal, unmodified characters.
pub(super) fn should_swallow_paste_tail(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if app.pending_paste_swallow.is_empty() {
        return false;
    }
    let expired = app.pending_paste_swallow_deadline.is_some_and(|deadline| std::time::Instant::now() > deadline);
    // A real `Ctrl+V` event right after a bypass paste means this
    // terminal passes the key through instead of owning it (unlike
    // Windows Terminal) -- the paste already happened, and letting the
    // key through would make the editor/field paste the same clipboard
    // a second time on its own. Doesn't end the swallow window: a
    // terminal that passes `Ctrl+V` through sends no flood at all, so
    // the queue just expires or ends on the next real keystroke.
    if !expired && modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('v' | 'V')) {
        return true;
    }
    let typed = match code {
        KeyCode::Char(c) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
        KeyCode::Enter if modifiers.is_empty() => Some('\n'),
        _ => None,
    };
    let matches_next = typed.is_some_and(|c| app.pending_paste_swallow.front() == Some(&c));
    if expired || !matches_next {
        app.pending_paste_swallow.clear();
        app.pending_paste_swallow_deadline = None;
        return false;
    }
    app.pending_paste_swallow.pop_front();
    if app.pending_paste_swallow.is_empty() {
        app.pending_paste_swallow_deadline = None;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    mod should_swallow_paste_tail_tests {
        use super::*;

        fn armed(chars: &str) -> (App, std::time::Instant) {
            let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
            app.pending_paste_swallow = chars.chars().collect();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            app.pending_paste_swallow_deadline = Some(deadline);
            (app, deadline)
        }

        #[test]
        fn nothing_pending_never_swallows() {
            let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
            assert!(!should_swallow_paste_tail(&mut app, KeyCode::Char('x'), KeyModifiers::NONE));
        }

        #[test]
        fn swallows_matching_chars_and_enter_in_order_until_the_queue_drains() {
            let (mut app, _) = armed("a\n");

            assert!(should_swallow_paste_tail(&mut app, KeyCode::Char('a'), KeyModifiers::NONE));
            assert_eq!(app.pending_paste_swallow, ['\n']);
            assert!(should_swallow_paste_tail(&mut app, KeyCode::Enter, KeyModifiers::NONE));
            assert!(app.pending_paste_swallow.is_empty());
            assert!(app.pending_paste_swallow_deadline.is_none(), "should clear its own deadline once the queue naturally drains");

            assert!(!should_swallow_paste_tail(&mut app, KeyCode::Char('z'), KeyModifiers::NONE), "a real keystroke after the queue is drained must not be swallowed");
        }

        #[test]
        fn a_key_that_is_not_a_char_or_enter_ends_the_swallow_without_eating_it() {
            let (mut app, _) = armed("hello");

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Left, KeyModifiers::NONE);

            assert!(!swallowed, "an unrelated key must never be silently discarded");
            assert!(app.pending_paste_swallow.is_empty(), "the mismatch should end the whole swallow window, not just skip this one key");
        }

        /// Real reported bug: `Ctrl+S` (save) and `Ctrl+Z` (undo the very
        /// paste this swallow exists for) right after a large paste were
        /// silently eaten instead of running -- both are `KeyCode::Char`
        /// too, and the swallow used to key off `code` alone, ignoring
        /// `modifiers` entirely. Windows Terminal's own flood only ever
        /// injects *plain* characters (no `Ctrl`/`Alt`), so a `Ctrl`-held
        /// `Char` must never be treated as part of it, even if its own
        /// letter happens to match the next expected flood character.
        #[test]
        fn a_ctrl_held_char_is_never_swallowed_even_mid_flood() {
            let (mut app, _) = armed("stuff");

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

            assert!(!swallowed, "Ctrl+S must reach the editor, not be eaten as flood tail");
            assert!(app.pending_paste_swallow.is_empty(), "a real shortcut mid-flood should end the swallow window entirely");
        }

        /// Real reported bug: pressing `Enter` several times right after a
        /// paste only registered several seconds late. Root cause: the
        /// swallow used to match on *shape* alone (any plain `Char`/bare
        /// `Enter`), so a real `Enter` typed while the flood was still
        /// mid-drain looked identical to one of the flood's own and got
        /// eaten too. Content-matching against the actual next expected
        /// character fixes this: a real `Enter` that doesn't match
        /// whatever the flood would send next must be handled immediately,
        /// not swallowed and delayed.
        #[test]
        fn a_real_keystroke_that_does_not_match_the_next_expected_character_is_handled_immediately() {
            let (mut app, _) = armed("hello world"); // next expected char is 'h', not Enter

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Enter, KeyModifiers::NONE);

            assert!(!swallowed, "a real Enter that doesn't match the flood's own next character must not be delayed");
            assert!(app.pending_paste_swallow.is_empty(), "the mismatch ends the swallow window entirely, so nothing further gets delayed either");
        }

        #[test]
        fn an_expired_deadline_ends_the_swallow_even_for_a_matching_key() {
            let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
            app.pending_paste_swallow = "a".chars().collect();
            app.pending_paste_swallow_deadline = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));

            let swallowed = should_swallow_paste_tail(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

            assert!(!swallowed, "past the safety-valve deadline, real typing should never be eaten even if it happens to match");
            assert!(app.pending_paste_swallow.is_empty());
        }
    }

    /// Real bug, found while extending the `Ctrl+V` bypass: in a
    /// terminal that passes `Ctrl+V` through instead of owning it, the
    /// real key event arriving right after the bypass paste would have
    /// made the field paste the same clipboard a second time.
    #[test]
    fn a_ctrl_v_right_after_a_bypass_paste_is_swallowed_without_ending_the_window() {
        let mut app = test_app(unique_scratch_dir("main-paste-swallow"));
        app.pending_paste_swallow = "ab".chars().collect();
        app.pending_paste_swallow_deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));

        assert!(should_swallow_paste_tail(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL));
        assert_eq!(app.pending_paste_swallow.len(), 2, "the flood (if any) is still expected afterward");
    }

    mod paste_routing_tests {
        use super::*;
        use crate::editor::{Editor, EditorKeymapMode};

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
}
