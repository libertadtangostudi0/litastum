use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};
use tracing::debug;

use crate::app::{App, Mode};
use crate::theme::Theme;
use crate::theme_menu::ThemeMenu;
use crate::ui::centered_rect;

/// F9's top menu. Currently just enough structure to reach the color
/// scheme picker through a "Settings" submenu (what was actually
/// asked for) — not Far Manager's full Left/Files/Commands/Options/
/// View/Right top-menu bar; see `TODO.md` for what a real one would
/// still need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLevel {
    Main,
    Settings,
}


impl MenuLevel {
    /// The items shown at this level, in order — `MainMenu::selected`
    /// indexes into this.
    pub fn items(self) -> &'static [&'static str] {
        match self {
            MenuLevel::Main => &["Settings"],
            MenuLevel::Settings => &["Color schemes"],
        }
    }
}


pub struct MainMenu {
    pub level: MenuLevel,
    pub selected: usize,
}


impl MainMenu {
    pub fn open() -> Self {
        Self { level: MenuLevel::Main, selected: 0 }
    }


    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    pub fn move_down(&mut self) {
        if self.selected + 1 < self.level.items().len() {
            self.selected += 1;
        }
    }


    /// Descends into the "Settings" submenu, resetting the cursor.
    pub fn enter_settings(&mut self) {
        self.level = MenuLevel::Settings;
        self.selected = 0;
    }


    /// Backs up one level. Returns `true` if it moved up a level (the
    /// caller stays in the menu); `false` if already at the top level
    /// (the caller should close the menu entirely).
    pub fn back(&mut self) -> bool {
        match self.level {
            MenuLevel::Settings => {
                self.level = MenuLevel::Main;
                self.selected = 0;
                true
            }
            MenuLevel::Main => false,
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    Up,
    Down,
    Select,
    Back,
    Ignore,
}


pub fn resolve(key: KeyEvent) -> MenuCommand {
    match key.code {
        KeyCode::Up => MenuCommand::Up,
        KeyCode::Down => MenuCommand::Down,
        KeyCode::Enter => MenuCommand::Select,
        KeyCode::Esc => MenuCommand::Back,
        _ => MenuCommand::Ignore,
    }
}


/// Key handling on the F9 top menu: `Up`/`Down` move, `Enter` descends
/// into a submenu or, at the deepest level ("Color schemes"), opens
/// `Mode::ThemeMenu`; `Esc` backs up one level, or closes the menu
/// entirely if already at the top. Moved here from `main.rs` so this
/// module owns its own state (`MainMenu`) *and* handling, the same way
/// `theme_menu.rs` does for its own picker.
pub fn handle_main_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::MainMenu(menu_state) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "main menu key");

    match command {
        MenuCommand::Up => menu_state.move_up(),
        MenuCommand::Down => menu_state.move_down(),
        MenuCommand::Back => {
            if !menu_state.back() {
                app.mode = Mode::Browsing;
            }
        }
        MenuCommand::Select => {
            let level = menu_state.level;
            match level {
                MenuLevel::Main => menu_state.enter_settings(),
                // Only one item at Settings level today ("Color
                // schemes"), so Select unconditionally opens it --
                // revisit once Settings grows more than one item.
                MenuLevel::Settings => app.mode = Mode::ThemeMenu(ThemeMenu::open()),
            }
        }
        MenuCommand::Ignore => {}
    }

    Ok(())
}


/// Renders the F9 top menu: whichever level's items are current
/// (`MenuLevel::items`), with the highlighted row picked out. Moved
/// here from `ui.rs` so this module owns state, key handling, *and*
/// rendering for its own popup.
pub fn draw_main_menu(frame: &mut Frame, area: Rect, menu: &MainMenu, theme: &Theme) {
    let items = menu.level.items();
    let height = (items.len() as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(30, height, area);

    frame.render_widget(Clear, popup);

    let title = match menu.level {
        MenuLevel::Main => " Menu ",
        MenuLevel::Settings => " Settings ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let style = if index == menu.selected {
                Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(*label, style)))
        })
        .collect();
    frame.render_widget(List::new(list_items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" open  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" back", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_down_clamped_at_last_item() {
        let mut menu = MainMenu::open(); // Main level, one item: "Settings"
        menu.move_down();
        assert_eq!(menu.selected, 0, "only one item at Main level");
    }

    #[test]
    fn move_up_clamped_at_first_item() {
        let mut menu = MainMenu::open();
        menu.move_up();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn enter_settings_switches_level_and_resets_cursor() {
        let mut menu = MainMenu::open();
        menu.selected = 0;
        menu.enter_settings();
        assert_eq!(menu.level, MenuLevel::Settings);
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.level.items(), &["Color schemes"]);
    }

    #[test]
    fn back_from_settings_returns_to_main() {
        let mut menu = MainMenu::open();
        menu.enter_settings();

        let stayed_in_menu = menu.back();

        assert!(stayed_in_menu);
        assert_eq!(menu.level, MenuLevel::Main);
    }

    #[test]
    fn back_from_main_signals_close() {
        let mut menu = MainMenu::open();
        assert!(!menu.back());
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn resolve_maps_keys() {
        assert_eq!(resolve(key(KeyCode::Up)), MenuCommand::Up);
        assert_eq!(resolve(key(KeyCode::Down)), MenuCommand::Down);
        assert_eq!(resolve(key(KeyCode::Enter)), MenuCommand::Select);
        assert_eq!(resolve(key(KeyCode::Esc)), MenuCommand::Back);
        assert_eq!(resolve(key(KeyCode::Char('z'))), MenuCommand::Ignore);
    }
}
