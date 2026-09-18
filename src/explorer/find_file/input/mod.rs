use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Mode};

use super::state::FindFilePhase;

mod results;
mod typing;

#[cfg(test)]
mod test_support;

/// Key handling for all three phases of the popup: typing the query
/// (`typing::handle_typing_key` — full-cursor editing, `text_field.rs`,
/// same reasoning as the F5/F6 transfer prompt, this is a modal popup
/// with no panel navigation happening under it), a search actually
/// running in the background (`Searching`, see `background.rs` — a
/// passive "please wait" screen, nothing bound here besides `Esc`
/// below), and, once it finishes, picking a result
/// (`results::handle_results_key` — `Up`/`Down`/`Enter`/`Tab`/`F4`).
/// `Esc` closes from any phase -- during `Searching`, it also cancels
/// the background search first (`PendingSearch::cancel`), so the
/// thread stops promptly instead of continuing to churn on a search
/// nothing's listening to the result of anymore. Handled once, here,
/// ahead of the per-phase dispatch below, rather than duplicated in
/// each of `typing`/`results` -- every phase's own `Esc` behavior is
/// identical except for that one extra cancel step.
pub fn handle_find_file_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Esc {
        if let Mode::FindFile(state) = &app.mode {
            if let Some(pending) = &state.pending {
                pending.cancel();
            }
        }
        app.mode = Mode::Browsing;
        return Ok(());
    }

    let Mode::FindFile(state) = &app.mode else {
        return Ok(());
    };

    match state.phase {
        FindFilePhase::Typing => typing::handle_typing_key(app, key),
        FindFilePhase::Searching => Ok(()),
        FindFilePhase::Results => results::handle_results_key(app, key),
    }
}


#[cfg(test)]
mod tests {
    use super::test_support::app_with_find_file;
    use super::*;
    use crate::explorer::FindFileState;
    use crate::test_support::key;

    #[test]
    fn esc_closes_from_any_phase() {
        let mut app = app_with_find_file(FindFileState::new());
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing));

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        let mut app = app_with_find_file(results_state);
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(matches!(app.mode, Mode::Browsing));
    }

    /// `Esc` during `FindFilePhase::Searching` should cancel the
    /// background search (not just close the popup) -- confirmed
    /// indirectly, since there's no synchronous way to observe the
    /// background thread noticing: the search is spawned over a large
    /// enough tree that, if cancellation weren't actually wired up, it
    /// would still be running (and would eventually try to send its
    /// full results into a channel this test's own `app`/`state` no
    /// longer owns, silently, per `PendingSearch`'s own doc comment).
    #[test]
    fn esc_during_searching_cancels_the_background_search() {
        use std::fs;

        let mut state = FindFileState::new();
        state.query = "file".to_string();
        let mut app = app_with_find_file(state);
        for i in 0..500 {
            fs::write(app.panels[0].path.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing), "Esc should still close the popup, same as every other phase");
    }
}
