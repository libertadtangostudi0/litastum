use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};

/// Everything this app tracks about Windows Terminal's own paste --
/// the physical `Ctrl+V` edge, the keystroke flood Windows Terminal
/// sends *after* this app has already pasted the clipboard itself, and
/// the characters that flood may have delivered *before* the physical
/// key press was even seen. See `paste_hotkey.rs`'s own module doc for
/// why any of this exists (Windows Terminal owns `Ctrl+V` and feeds the
/// clipboard in as simulated keystrokes at ~7-8ms each), and
/// `event_loop::paste::try_intercept_paste_hotkey` for the one place
/// that drives it.
///
/// Compiled on every platform: `should_swallow`/`record_typed_key` run
/// for every key everywhere, they just never find anything to do unless
/// the Windows-only half (`ctrl_v_just_pressed`/`expect`) has armed
/// something first.
#[derive(Default)]
pub struct PasteFlood {
    /// Real physical `Ctrl+V` state as of the previous poll -- only used
    /// to detect a *fresh* press (edge-triggered), not "is it held."
    #[cfg(windows)]
    ctrl_v_held: bool,
    /// The characters the flood is still expected to deliver, front =
    /// next -- `'\n'` means a bare `Enter`, anything else that exact
    /// `Char`. Empty means nothing pending, the normal state.
    /// `should_swallow` discards an incoming key only when it actually
    /// *matches* the front, popping it; a real keystroke that doesn't
    /// match clears the whole queue instead of being swallowed.
    expected: VecDeque<char>,
    /// Safety valve for `expected`, not its primary stop condition
    /// (that's the queue draining naturally or a mismatched key clearing
    /// it): guards against the flood simply never arriving, or stalling,
    /// so a still-correct but very slow queue can't keep swallowing
    /// forever.
    deadline: Option<Instant>,
    /// The last few characters actually typed through ordinary key
    /// handling, with when they arrived -- lets `not_yet_delivered`
    /// notice that the flood already delivered the start of the paste
    /// before the physical key press was seen (see
    /// `already_typed_prefix_len`'s own doc comment for the real report).
    /// Cleared by any other key, since characters typed before it are no
    /// longer right before where a paste would land.
    recently_typed: VecDeque<(char, Instant)>,
}


/// How long a typed character stays eligible to be recognized as the
/// head of a paste flood -- the flood runs at ~7-8ms per character, and
/// the physical-key poll only ever lags it by a loop iteration or so,
/// so this is generous.
#[cfg(windows)]
const RECENTLY_TYPED_WINDOW: Duration = Duration::from_millis(500);

/// More than enough for the few characters the flood can get ahead of
/// the physical-key poll.
const RECENTLY_TYPED_CAP: usize = 64;


impl PasteFlood {
    /// Whether a real physical `Ctrl+V` press started since the last
    /// call -- `true` once per press, never repeatedly while held.
    #[cfg(windows)]
    pub fn ctrl_v_just_pressed(&mut self) -> bool {
        let held = super::paste_hotkey::ctrl_v_physically_down();
        let just_pressed = held && !self.ctrl_v_held;
        self.ctrl_v_held = held;
        just_pressed
    }

    /// The part of `text` (line-break `\r`s dropped, the way the flood
    /// never sends them either) that the flood hasn't *already*
    /// delivered through ordinary typing -- what should actually be
    /// pasted now, and what the rest of the flood will be. Forgets the
    /// recently typed characters either way; they've been accounted for.
    #[cfg(windows)]
    pub fn not_yet_delivered(&mut self, text: &str) -> String {
        let chars: Vec<char> = text.chars().filter(|&c| c != '\r').collect();
        let now = Instant::now();
        let recent: Vec<char> = self.recently_typed.iter().filter(|(_, at)| now.duration_since(*at) <= RECENTLY_TYPED_WINDOW).map(|&(c, _)| c).collect();
        self.recently_typed.clear();
        chars[already_typed_prefix_len(&recent, &chars)..].iter().collect()
    }

    /// Expects the flood to deliver `text` next, right after this app
    /// already pasted it itself, so `should_swallow` can discard it
    /// instead of typing the same text a second time -- for a single-line
    /// field, that also keeps the flood's own `Enter` keystrokes (one per
    /// pasted line break) from submitting the field.
    ///
    /// **Extends the queue, never overwrites it.** Real reported bug:
    /// pasting again quickly (before the *first* paste's own flood had
    /// finished arriving) left a visible, slow, character-by-character
    /// typing delay afterward -- overwriting the queue with just the
    /// second paste's own text threw away whatever was left of the first
    /// paste's still-incoming flood, so those leftover characters no
    /// longer matched anything expected and got typed as real (if
    /// nonsensical) input, one throttled keystroke at a time. Appending
    /// keeps a still-pending tail from an earlier paste being discarded
    /// first, in the same order the two floods actually arrive in
    /// (Windows Terminal processes one paste's own injection before
    /// starting the next).
    #[cfg(windows)]
    pub fn expect(&mut self, text: &str) {
        self.expected.extend(text.chars().filter(|&c| c != '\r'));
        // Generous relative to the ~7-8ms/char rate this was actually
        // measured at -- this only exists to eventually give up if the
        // flood never arrives, not to race it. Recomputed from the whole
        // (possibly just-extended) queue, so a second paste's own budget
        // still covers whatever's left of an earlier one ahead of it.
        let budget_ms = (self.expected.len() as u64).saturating_mul(100).max(2000);
        self.deadline = Some(Instant::now() + Duration::from_millis(budget_ms));
    }

    /// Records one key that went through ordinary key handling
    /// (`event_loop::keys`) -- a plain character is remembered, a bare
    /// `Enter` as `'\n'` (how the flood sends a line break); any other key
    /// forgets everything.
    pub fn record_typed_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let typed = match code {
            KeyCode::Char(c) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => c,
            KeyCode::Enter if modifiers.is_empty() => '\n',
            _ => {
                self.recently_typed.clear();
                return;
            }
        };
        self.recently_typed.push_back((typed, Instant::now()));
        if self.recently_typed.len() > RECENTLY_TYPED_CAP {
            self.recently_typed.pop_front();
        }
    }

    /// `true` means "this key is the flood's next expected character --
    /// discard it rather than typing the same text a second time." Pops
    /// the queue on every swallow, and clears it entirely (without
    /// swallowing this key) once it drains naturally, the deadline
    /// passes, or this key simply isn't what was expected next.
    ///
    /// **Matches by content, not just by shape** -- reported directly:
    /// pressing `Enter` several times right after a paste only
    /// registered with a 5-10 second delay. An earlier version only
    /// checked whether a key was *character-shaped* (a plain `Char` or
    /// bare `Enter`) against a plain decrementing counter -- which also
    /// matches ordinary keystrokes typed *during* the still-draining
    /// window (a real `Enter` looks identical in shape to a flood
    /// `Enter`), so genuine typing got silently eaten and delayed.
    /// Comparing against the actual next expected character means a real
    /// keystroke that doesn't match (nearly always) is handled right away.
    ///
    /// Modifiers matter too, from an earlier report: `Ctrl+S`/`Ctrl+Z`
    /// must never match regardless of their `Char` code -- the flood only
    /// ever injects the pasted text's own literal, unmodified characters.
    pub fn should_swallow(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        if self.expected.is_empty() {
            return false;
        }
        let expired = self.deadline.is_some_and(|deadline| Instant::now() > deadline);
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
        let matches_next = typed.is_some_and(|c| self.expected.front() == Some(&c));
        if expired || !matches_next {
            self.expected.clear();
            self.deadline = None;
            return false;
        }
        self.expected.pop_front();
        if self.expected.is_empty() {
            self.deadline = None;
        }
        true
    }
}


/// How many leading characters of `text` were already typed as the
/// very latest entries of `recent` -- the longest `k` such that
/// `recent`'s last `k` characters are exactly `text`'s first `k`.
///
/// Reported directly, with screenshots: pasting "Тесты добавлены:" into
/// the editor's `Ctrl+F` box produced "ТТесты добавлены:есты добавл...".
/// A race, not a logic slip in any one place: Windows Terminal starts
/// injecting its keystroke flood the instant `Ctrl+V` goes down, and
/// the first flood character can be read and typed normally before the
/// next `GetAsyncKeyState` poll ever sees the physical key. The bypass
/// then pasted the *whole* clipboard after it (doubling the "Т") and
/// armed the swallow expecting a flood starting with "Т" -- the flood's
/// actual next character, "е", didn't match, the swallow gave up, and
/// the rest of the flood got typed as ordinary input. Skipping whatever
/// the flood already delivered -- pasting and expecting only the rest
/// -- makes both come out right.
#[cfg(any(windows, test))]
fn already_typed_prefix_len(recent: &[char], text: &[char]) -> usize {
    (1..=recent.len().min(text.len())).rev().find(|&k| recent[recent.len() - k..] == text[..k]).unwrap_or(0)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn expecting(chars: &str) -> PasteFlood {
        PasteFlood { expected: chars.chars().collect(), deadline: Some(Instant::now() + Duration::from_secs(5)), ..PasteFlood::default() }
    }

    mod should_swallow_tests {
        use super::*;

        #[test]
        fn nothing_pending_never_swallows() {
            assert!(!PasteFlood::default().should_swallow(KeyCode::Char('x'), KeyModifiers::NONE));
        }

        #[test]
        fn swallows_matching_chars_and_enter_in_order_until_the_queue_drains() {
            let mut flood = expecting("a\n");

            assert!(flood.should_swallow(KeyCode::Char('a'), KeyModifiers::NONE));
            assert_eq!(flood.expected, ['\n']);
            assert!(flood.should_swallow(KeyCode::Enter, KeyModifiers::NONE));
            assert!(flood.expected.is_empty());
            assert!(flood.deadline.is_none(), "should clear its own deadline once the queue naturally drains");

            assert!(!flood.should_swallow(KeyCode::Char('z'), KeyModifiers::NONE), "a real keystroke after the queue is drained must not be swallowed");
        }

        #[test]
        fn a_key_that_is_not_a_char_or_enter_ends_the_swallow_without_eating_it() {
            let mut flood = expecting("hello");

            assert!(!flood.should_swallow(KeyCode::Left, KeyModifiers::NONE), "an unrelated key must never be silently discarded");
            assert!(flood.expected.is_empty(), "the mismatch should end the whole swallow window, not just skip this one key");
        }

        /// Real reported bug: `Ctrl+S` (save) and `Ctrl+Z` (undo the very
        /// paste this swallow exists for) right after a large paste were
        /// silently eaten instead of running -- both are `KeyCode::Char`
        /// too, and the swallow used to key off `code` alone, ignoring
        /// `modifiers` entirely.
        #[test]
        fn a_ctrl_held_char_is_never_swallowed_even_mid_flood() {
            let mut flood = expecting("stuff");

            assert!(!flood.should_swallow(KeyCode::Char('s'), KeyModifiers::CONTROL), "Ctrl+S must reach the editor, not be eaten as flood tail");
            assert!(flood.expected.is_empty(), "a real shortcut mid-flood should end the swallow window entirely");
        }

        /// Real reported bug: pressing `Enter` several times right after a
        /// paste only registered several seconds late, because the swallow
        /// matched on *shape* alone.
        #[test]
        fn a_real_keystroke_that_does_not_match_the_next_expected_character_is_handled_immediately() {
            let mut flood = expecting("hello world"); // next expected char is 'h', not Enter

            assert!(!flood.should_swallow(KeyCode::Enter, KeyModifiers::NONE), "a real Enter that doesn't match the flood's own next character must not be delayed");
            assert!(flood.expected.is_empty(), "the mismatch ends the swallow window entirely, so nothing further gets delayed either");
        }

        #[test]
        fn an_expired_deadline_ends_the_swallow_even_for_a_matching_key() {
            let mut flood = PasteFlood { expected: "a".chars().collect(), deadline: Some(Instant::now() - Duration::from_secs(1)), ..PasteFlood::default() };

            assert!(!flood.should_swallow(KeyCode::Char('a'), KeyModifiers::NONE), "past the safety-valve deadline, real typing should never be eaten even if it happens to match");
            assert!(flood.expected.is_empty());
        }

        /// Real bug, found while extending the `Ctrl+V` bypass: in a
        /// terminal that passes `Ctrl+V` through instead of owning it, the
        /// real key event arriving right after the bypass paste would have
        /// made the field paste the same clipboard a second time.
        #[test]
        fn a_ctrl_v_right_after_a_bypass_paste_is_swallowed_without_ending_the_window() {
            let mut flood = expecting("ab");

            assert!(flood.should_swallow(KeyCode::Char('v'), KeyModifiers::CONTROL));
            assert_eq!(flood.expected.len(), 2, "the flood (if any) is still expected afterward");
        }
    }

    mod already_typed_prefix_tests {
        use super::*;

        fn chars(text: &str) -> Vec<char> {
            text.chars().collect()
        }

        /// The real report: the flood's first character ("Т") was typed
        /// before the bypass saw `Ctrl+V` -- only the rest should be
        /// pasted and expected from the flood.
        #[test]
        fn a_flood_head_already_typed_is_recognized() {
            assert_eq!(already_typed_prefix_len(&chars("xТ"), &chars("Тесты добавлены:")), 1);
            assert_eq!(already_typed_prefix_len(&chars("Тес"), &chars("Тесты")), 3);
        }

        #[test]
        fn nothing_typed_or_no_overlap_means_paste_everything() {
            assert_eq!(already_typed_prefix_len(&[], &chars("abc")), 0);
            assert_eq!(already_typed_prefix_len(&chars("xyz"), &chars("abc")), 0);
        }

        /// Only the *latest* typed characters count -- an older, earlier
        /// match that isn't right before the paste point doesn't.
        #[test]
        fn only_the_most_recent_characters_can_match() {
            assert_eq!(already_typed_prefix_len(&chars("ab!"), &chars("abc")), 0);
        }

        #[test]
        fn record_typed_key_forgets_everything_on_any_other_key() {
            let mut flood = PasteFlood::default();
            flood.record_typed_key(KeyCode::Char('a'), KeyModifiers::NONE);
            flood.record_typed_key(KeyCode::Enter, KeyModifiers::NONE);
            assert_eq!(flood.recently_typed.iter().map(|&(c, _)| c).collect::<String>(), "a\n");

            flood.record_typed_key(KeyCode::Left, KeyModifiers::NONE);
            assert!(flood.recently_typed.is_empty(), "text typed before a cursor move isn't where a paste lands");

            flood.record_typed_key(KeyCode::Char('s'), KeyModifiers::CONTROL);
            assert!(flood.recently_typed.is_empty(), "a shortcut is not typed text");
        }
    }
}
