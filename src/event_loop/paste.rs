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
/// let `run()` redraw) only when something actually happened --a fresh
/// press outside the one context this has a fast path for (the
/// built-in editor, in ordinary typing mode -- `Editor::is_plain_standard_typing`)
/// is deliberately left completely alone, no swallow armed, so
/// `crossterm`'s normal (slow) event flow handles it exactly as before
/// anywhere else -- extending this to the command line/other popups is
/// real future work, not something this fix reaches for speculatively.
///
/// See `paste_hotkey.rs`'s own module doc comment for why a real
/// terminal `Ctrl+V` needs this bypass in the first place, and for how
/// the mismatch between "read the clipboard right now" and "Windows
/// Terminal's own flood is still coming, one keystroke at a time" is
/// resolved: `app.pending_paste_swallow` is armed here (to the pasted
/// text's own keystroke-equivalent length) so `handle_key_event` can
/// silently discard that flood once it actually arrives, instead of
/// typing the same text a second time right after this already pasted
/// it once, instantly.
#[cfg(windows)]
pub(super) fn try_intercept_paste_hotkey(app: &mut App) -> Result<bool> {
    let held = crate::paste_hotkey::ctrl_v_physically_down();
    let just_pressed = held && !app.ctrl_v_physically_held;
    app.ctrl_v_physically_held = held;
    if !just_pressed {
        return Ok(false);
    }

    let Mode::Editing(editor) = &mut app.mode else {
        return Ok(false);
    };
    if !editor.is_plain_standard_typing() {
        return Ok(false);
    }
    let text = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        Ok(text) => text,
        Err(_) => return Ok(false),
    };
    if text.is_empty() {
        return Ok(false);
    }

    editor.paste_text(&text);
    arm_paste_swallow(app, &text);
    Ok(true)
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
/// fix. While the built-in editor is open, hands `text` straight to
/// `Editor::paste_text` -- the same fast, O(text length) splice
/// `Ctrl+V` itself uses (`editor::fast_paste_from_clipboard`), just fed
/// from this event's own text instead of a fresh clipboard read (the
/// terminal already handed it to us; reading the clipboard again would
/// just be redundant, and could even race a clipboard change between
/// the copy and this paste actually arriving).
///
/// Every other mode has no equivalent fast path of its own (the
/// always-live command line, Find file's fields, the transfer popup,
/// ...) -- replayed as ordinary per-character key presses through the
/// exact same `dispatch_key_event` a real keystroke would take, just
/// looped here with no redraw in between rather than arriving one at a
/// time over the wire with a full redraw after each (which is what
/// "no bracketed paste" looked like before this existed, for *every*
/// mode, not just the editor). A newline in the pasted text is skipped
/// rather than replayed as `Enter` -- these fields are single-line, and
/// forwarding it could submit a command/form the user never meant to
/// trigger this instant, the same "may execute unexpected commands"
/// concern Windows Terminal's own multi-line-paste warning is about.
pub(super) fn handle_paste_event(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, text: &str) -> Result<()> {
    if let Mode::Editing(editor) = &mut app.mode {
        editor.paste_text(text);
        return Ok(());
    }

    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        dispatch_key_event(app, crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), terminal)?;
    }
    Ok(())
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
}
