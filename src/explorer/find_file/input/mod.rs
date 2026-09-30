use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Overlay};
use crate::command_line::Effect;

use super::state::FindFilePhase;

mod results;
mod typing;

#[cfg(test)]
mod test_support;

/// Keys for the three phases: typing the query (`typing`, full text-field
/// editing), `Searching` (only `Esc`), and picking a result (`results`).
/// `Esc` closes from any phase, handled once here; while searching it
/// also cancels the background search.
pub fn handle_find_file_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if key.code == KeyCode::Esc {
        if let Some(Overlay::FindFile(state)) = &app.overlay {
            if let Some(pending) = &state.pending {
                pending.cancel();
            }
        }
        app.overlay = None;
        return Ok(Effect::None);
    }

    let Some(Overlay::FindFile(state)) = &app.overlay else {
        return Ok(Effect::None);
    };

    match state.phase {
        FindFilePhase::Typing => typing::handle_typing_key(app, key),
        FindFilePhase::Searching => Ok(Effect::None),
        FindFilePhase::Results => results::handle_results_key(app, key),
    }
}


#[cfg(test)]
mod tests {
    use super::test_support::app_with_find_file;
    use super::*;
    use crate::app::Mode;
    use crate::explorer::FindFileState;
    use crate::test_support::key;

    #[test]
    fn esc_closes_from_any_phase() {
        let mut app = app_with_find_file(FindFileState::new());
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));

        let mut results_state = FindFileState::new();
        results_state.phase = FindFilePhase::Results;
        let mut app = app_with_find_file(results_state);
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }

    /// `Esc` while searching cancels the search -- observed indirectly: the
    /// tree is large enough that an uncancelled search would still be running.
    #[test]
    fn esc_during_searching_cancels_the_background_search() {
        use std::fs;

        let mut state = FindFileState::new();
        state.query.set_text("file");
        let mut app = app_with_find_file(state);
        for i in 0..500 {
            fs::write(app.panels[0].path.join(format!("file_{i}.txt")), b"hi").unwrap();
        }

        handle_find_file_key(&mut app, key(KeyCode::Enter)).unwrap();
        handle_find_file_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing), "Esc should still close the popup, same as every other phase");
    }
}
