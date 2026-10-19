//! F9 -> Options -> Trusted links: the Markdown links trusted for good
//! (`trust`), newest first. `Del`/`F8` forgets the highlighted one -- it
//! asks again next time it's opened.

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Overlay};
use crate::command_line::Effect;

use super::trust::{load_trusted, save_trusted};


pub struct TrustedLinksMenu {
    /// Newest first, as shown.
    links: Vec<String>,
    selected: usize,
}


impl TrustedLinksMenu {
    pub fn open() -> Self {
        let mut links = load_trusted();
        links.reverse();
        Self { links, selected: 0 }
    }


    pub fn links(&self) -> &[String] {
        &self.links
    }


    pub fn selected(&self) -> usize {
        self.selected
    }


    pub fn move_up(&mut self) {
        crate::list_cursor::move_up(&mut self.selected);
    }


    pub fn move_down(&mut self) {
        crate::list_cursor::move_down(&mut self.selected, self.links.len());
    }


    fn forget_selected(&mut self) {
        if self.selected >= self.links.len() {
            return;
        }
        self.links.remove(self.selected);
        self.selected = self.selected.min(self.links.len().saturating_sub(1));
        let oldest_first: Vec<String> = self.links.iter().rev().cloned().collect();
        save_trusted(&oldest_first);
    }
}


/// `Up`/`Down` move, `Del`/`F8` forget the highlighted link, `Esc` or
/// `Enter` close.
pub fn handle_trusted_links_menu_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::TrustedLinks(menu)) = &mut app.overlay else {
        return Ok(Effect::None);
    };
    match key.code {
        KeyCode::Up => menu.move_up(),
        KeyCode::Down => menu.move_down(),
        KeyCode::Delete | KeyCode::F(8) => {
            debug!("trusted links: forgetting one");
            menu.forget_selected();
        }
        KeyCode::Esc | KeyCode::Enter => app.overlay = None,
        _ => {}
    }
    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_menu(trusted: &[&str]) -> App {
        save_trusted(&trusted.iter().map(|link| link.to_string()).collect::<Vec<_>>());
        let mut app = test_app(unique_scratch_dir("trusted-links-menu"));
        app.overlay = Some(Overlay::TrustedLinks(TrustedLinksMenu::open()));
        app
    }

    fn shown(app: &App) -> Vec<String> {
        let Some(Overlay::TrustedLinks(menu)) = &app.overlay else { panic!("expected the trusted links menu") };
        menu.links().to_vec()
    }

    #[test]
    fn lists_the_newest_first() {
        let app = app_with_menu(&["old", "new"]);
        assert_eq!(shown(&app), vec!["new", "old"]);
    }

    /// Requested: a trusted link can be taken off the list -- it asks again.
    #[test]
    fn del_forgets_the_highlighted_link_for_good() {
        let mut app = app_with_menu(&["a", "b", "c"]);
        handle_trusted_links_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        handle_trusted_links_menu_key(&mut app, key(KeyCode::Delete)).unwrap();

        assert_eq!(shown(&app), vec!["c", "a"]);
        assert_eq!(load_trusted(), vec!["a", "c"], "saved, oldest first");
    }

    #[test]
    fn forgetting_the_last_row_moves_the_highlight_up() {
        let mut app = app_with_menu(&["a", "b"]);
        handle_trusted_links_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        handle_trusted_links_menu_key(&mut app, key(KeyCode::F(8))).unwrap();

        let Some(Overlay::TrustedLinks(menu)) = &app.overlay else { unreachable!() };
        assert_eq!(menu.selected(), 0);
        assert_eq!(menu.links(), &["b".to_string()]);
    }

    #[test]
    fn del_on_an_empty_list_does_nothing() {
        let mut app = app_with_menu(&[]);
        handle_trusted_links_menu_key(&mut app, key(KeyCode::Delete)).unwrap();
        assert!(shown(&app).is_empty());
    }

    #[test]
    fn esc_closes() {
        let mut app = app_with_menu(&["a"]);
        handle_trusted_links_menu_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(app.overlay.is_none());
    }
}
