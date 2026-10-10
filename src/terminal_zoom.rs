//! A zoom of its own per screen in Windows Terminal (requested: the
//! panels, the editor, Compare and the resolver each keep theirs, as in
//! litastum's window). Windows Terminal owns `Ctrl`+`+`/`-`/`0` -- the app
//! never gets them, and nothing lets an app set the terminal's font size.
//! So the presses are watched (`windows_terminal::zoom_keys`) and counted
//! as steps of the screen they were made on, and on a switch of screen
//! litastum presses the keys itself until the terminal is at that screen's
//! steps; at exit it presses them back. Saved for the user in
//! `config.json` (`terminal_zoom`), and where the terminal's tab is now
//! (`terminal_zoom_now`), so a litastum that ended without putting it
//! back doesn't count from the wrong size. History: docs/history/launching.md.

use std::collections::BTreeMap;


/// The steps per screen, and where the terminal is now -- in steps from
/// its own size (`Ctrl`+`0`).
#[derive(Debug)]
pub struct TerminalZoom {
    steps: BTreeMap<String, i32>,
    screen: String,
    /// The terminal's steps now.
    applied: i32,
    /// Changed since last saved.
    unsaved: bool,
}

impl TerminalZoom {
    /// Starting on the panels' screen (`main`), with the terminal at
    /// `applied` steps.
    pub fn new(steps: BTreeMap<String, i32>, applied: i32) -> Self {
        Self { steps, screen: "main".to_string(), applied, unsaved: false }
    }

    /// The screen shown now (`event_loop::screen_name`).
    pub fn set_screen(&mut self, screen: &str) {
        if self.screen != screen {
            self.screen = screen.to_string();
        }
    }

    /// The user pressed `Ctrl`+`+` (`by` > 0) or `Ctrl`+`-` that many
    /// times: the terminal made the steps already; they're the shown
    /// screen's.
    pub fn stepped(&mut self, by: i32) {
        if by == 0 {
            return;
        }
        self.applied += by;
        self.set_steps(self.applied);
    }

    /// The user pressed `Ctrl`+`0`: the terminal is back at its own size,
    /// and so is the shown screen.
    pub fn reset(&mut self) {
        self.applied = 0;
        self.set_steps(0);
    }

    fn set_steps(&mut self, steps: i32) {
        self.steps.insert(self.screen.clone(), steps);
        self.unsaved = true;
    }

    /// The presses that bring the terminal to the shown screen's steps
    /// (`+` up, `-` down) -- a screen with none of its own takes the
    /// panels'.
    pub fn presses_needed(&self) -> i32 {
        let target = self.steps.get(&self.screen).or_else(|| self.steps.get("main")).copied().unwrap_or(0);
        target - self.applied
    }

    /// The presses back to the terminal's own size, for exit.
    pub fn presses_to_restore(&self) -> i32 {
        -self.applied
    }

    /// litastum pressed `presses` itself.
    pub fn pressed(&mut self, presses: i32) {
        if presses != 0 {
            self.applied += presses;
            self.unsaved = true;
        }
    }

    /// The steps per screen and the terminal's now, to save once changed.
    pub fn take_unsaved(&mut self) -> Option<(BTreeMap<String, i32>, i32)> {
        if !self.unsaved {
            return None;
        }
        self.unsaved = false;
        Some((self.steps.clone(), self.applied))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Requested: Compare zooms without the panels.
    #[test]
    fn each_screen_keeps_its_own_steps() {
        let mut zoom = TerminalZoom::new(BTreeMap::new(), 0);
        zoom.set_screen("compare");
        zoom.stepped(2);
        assert_eq!(zoom.presses_needed(), 0, "the terminal made them already");

        zoom.set_screen("main");
        assert_eq!(zoom.presses_needed(), -2, "back to the panels' size");
        zoom.pressed(-2);
        zoom.set_screen("compare");
        assert_eq!(zoom.presses_needed(), 2);
    }

    #[test]
    fn a_screen_with_no_zoom_of_its_own_takes_the_panels() {
        let mut zoom = TerminalZoom::new(BTreeMap::new(), 0);
        zoom.stepped(-1);
        zoom.set_screen("editor");
        assert_eq!(zoom.presses_needed(), 0, "already at the panels' size");
    }

    /// Zooming on a screen leaves the others' steps alone.
    #[test]
    fn a_zoom_on_one_screen_keeps_the_others() {
        let mut zoom = TerminalZoom::new(BTreeMap::from([("main".to_string(), -2), ("compare".to_string(), 3)]), -2);
        zoom.stepped(-1);
        zoom.set_screen("compare");
        assert_eq!(zoom.presses_needed(), 6, "-3 to 3");
        zoom.pressed(6);
        zoom.set_screen("main");
        assert_eq!(zoom.presses_needed(), -6);
    }

    #[test]
    fn ctrl_0_brings_the_terminal_and_the_screen_back() {
        let mut zoom = TerminalZoom::new(BTreeMap::new(), 0);
        zoom.set_screen("compare");
        zoom.stepped(1);
        zoom.reset();
        assert_eq!(zoom.take_unsaved().map(|(steps, _)| steps["compare"]), Some(0));
        assert_eq!(zoom.presses_to_restore(), 0);
    }

    /// The terminal may still be zoomed from a litastum that ended without
    /// putting it back: counted from there, not from its own size.
    #[test]
    fn a_terminal_left_zoomed_is_counted_from_where_it_is() {
        let zoom = TerminalZoom::new(BTreeMap::from([("main".to_string(), -2)]), -2);
        assert_eq!(zoom.presses_needed(), 0, "already there");
        assert_eq!(zoom.presses_to_restore(), 2);
    }

    #[test]
    fn saved_steps_apply_and_exit_restores() {
        let mut zoom = TerminalZoom::new(BTreeMap::from([("main".to_string(), -1), ("compare".to_string(), 2)]), 0);
        assert_eq!(zoom.presses_needed(), -1);
        zoom.pressed(-1);
        zoom.set_screen("compare");
        zoom.pressed(zoom.presses_needed());
        assert_eq!(zoom.presses_to_restore(), -2);
        assert_eq!(zoom.take_unsaved().map(|(_, now)| now), Some(2), "where the terminal is, saved");
        assert_eq!(zoom.take_unsaved(), None);
    }
}
