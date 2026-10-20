//! F9 -> Options -> Hotkeys: every key binding in a scrollable table --
//! the key, what it does, and when, if only in some state. For now the
//! panels' (`command_line::browser_hotkeys`); rebinding comes next.

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Overlay};
use crate::command_line::Effect;

/// Rows `PgUp`/`PgDn` move by.
const PAGE: usize = 10;


/// One line of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyRow {
    pub keys: String,
    pub action: &'static str,
    /// Empty when the key always does this.
    pub when: &'static str,
}


pub struct HotkeysMenu {
    rows: Vec<HotkeyRow>,
    selected: usize,
}


impl HotkeysMenu {
    pub fn open() -> Self {
        Self { rows: crate::command_line::browser_hotkeys(), selected: 0 }
    }


    pub fn rows(&self) -> &[HotkeyRow] {
        &self.rows
    }


    pub fn selected(&self) -> usize {
        self.selected
    }


    #[cfg(test)]
    pub fn select_last_for_tests(&mut self) {
        self.move_by(isize::MAX);
    }


    fn move_by(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }
}


/// `Up`/`Down`, `PgUp`/`PgDn`, `Home`/`End` move; `Esc`/`Enter` close.
pub fn handle_hotkeys_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::Hotkeys(menu)) = &mut app.overlay else {
        return Ok(Effect::None);
    };
    match key.code {
        KeyCode::Up => menu.move_by(-1),
        KeyCode::Down => menu.move_by(1),
        KeyCode::PageUp => menu.move_by(-(PAGE as isize)),
        KeyCode::PageDown => menu.move_by(PAGE as isize),
        KeyCode::Home => menu.selected = 0,
        KeyCode::End => menu.move_by(isize::MAX),
        KeyCode::Esc | KeyCode::Enter => app.overlay = None,
        _ => {}
    }
    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_table() -> App {
        let mut app = test_app(unique_scratch_dir("hotkeys"));
        app.overlay = Some(Overlay::Hotkeys(HotkeysMenu::open()));
        app
    }

    fn selected(app: &App) -> (usize, usize) {
        let Some(Overlay::Hotkeys(menu)) = &app.overlay else { panic!("expected the hotkeys table") };
        (menu.selected(), menu.rows().len())
    }

    #[test]
    fn the_table_scrolls_and_stops_at_both_ends() {
        let mut app = app_with_table();

        handle_hotkeys_key(&mut app, key(KeyCode::Up)).unwrap();
        assert_eq!(selected(&app).0, 0);
        handle_hotkeys_key(&mut app, key(KeyCode::PageDown)).unwrap();
        assert_eq!(selected(&app).0, PAGE);
        handle_hotkeys_key(&mut app, key(KeyCode::End)).unwrap();
        let (at, len) = selected(&app);
        assert_eq!(at, len - 1);
        handle_hotkeys_key(&mut app, key(KeyCode::Down)).unwrap();
        assert_eq!(selected(&app).0, len - 1);
        handle_hotkeys_key(&mut app, key(KeyCode::Home)).unwrap();
        assert_eq!(selected(&app).0, 0);
    }

    #[test]
    fn esc_closes() {
        let mut app = app_with_table();
        handle_hotkeys_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(app.overlay.is_none());
    }
}
