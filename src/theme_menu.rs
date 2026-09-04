use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use tracing::debug;

use crate::app::{App, Mode};
use crate::config;
use crate::theme::Theme;
use crate::ui::centered_rect;


/// State for the F9 "pick a color scheme" popup — a minimal analog of
/// Far Manager's F9 menu, scoped to just color schemes for now (a real
/// top-menu bar — Left/Files/Commands/Options/... — is a much bigger
/// feature; see `TODO.md`).
pub struct ThemeMenu {
    /// Filename stems of every `themes/*.json` file found in the
    /// config dir at the moment F9 was pressed — a snapshot, not
    /// live-refreshed while the menu stays open.
    pub themes: Vec<String>,
    pub selected: usize,
}


impl ThemeMenu {
    /// Scans `<config_dir>/themes/` for theme files. Empty (not an
    /// error) if there's no config dir or no `themes/` subdirectory yet
    /// — same "nothing configured" case `config.rs` already treats as
    /// normal, not a failure to report.
    pub fn open() -> Self {
        Self { themes: config::list_theme_names(), selected: 0 }
    }


    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    pub fn move_down(&mut self) {
        if self.selected + 1 < self.themes.len() {
            self.selected += 1;
        }
    }


    pub fn selected_theme(&self) -> Option<&str> {
        self.themes.get(self.selected).map(String::as_str)
    }
}


/// A user-triggered action on the theme-picker popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMenuCommand {
    Up,
    Down,
    /// Apply the highlighted theme as both the interface and editor
    /// theme — the common case (one scheme driving everything).
    ApplyBoth,
    /// Apply as the interface theme only, leaving `editor_theme`
    /// untouched — for anyone who wants the two independent, matching
    /// how Far Manager itself keeps them (see
    /// `.claude/rules/litastum-theming.md`).
    ApplyInterfaceOnly,
    ApplyEditorOnly,
    Close,
    Ignore,
}


/// Resolves a raw key press on the theme-picker popup.
pub fn resolve(key: KeyEvent) -> ThemeMenuCommand {
    match key.code {
        KeyCode::Up => ThemeMenuCommand::Up,
        KeyCode::Down => ThemeMenuCommand::Down,
        KeyCode::Enter => ThemeMenuCommand::ApplyBoth,
        KeyCode::Char('i' | 'I') => ThemeMenuCommand::ApplyInterfaceOnly,
        KeyCode::Char('e' | 'E') => ThemeMenuCommand::ApplyEditorOnly,
        KeyCode::Esc => ThemeMenuCommand::Close,
        _ => ThemeMenuCommand::Ignore,
    }
}


/// Key handling on the F9 color-scheme picker: `Enter` applies the
/// highlighted theme as both interface and editor theme, `I`/`E` apply
/// just one side, `Esc` closes without changing anything. Applying
/// updates `app.theme`/`app.syntax_theme` immediately — no restart —
/// and persists the choice to `config.json` on a best-effort basis
/// (`config.rs` logs and carries on if that write fails; the live
/// preview still applies). Moved here from `main.rs` so this module
/// owns its own state (`ThemeMenu`) *and* handling.
pub fn handle_theme_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ThemeMenu(menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "theme menu key");

    match command {
        ThemeMenuCommand::Up => menu.move_up(),
        ThemeMenuCommand::Down => menu.move_down(),
        ThemeMenuCommand::Close => app.mode = Mode::Browsing,
        ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyInterfaceOnly | ThemeMenuCommand::ApplyEditorOnly => {
            let Some(name) = menu.selected_theme().map(str::to_string) else {
                return Ok(());
            };
            if matches!(command, ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyInterfaceOnly) {
                if let Some(theme) = config::set_interface_theme(&name) {
                    app.theme = theme;
                }
            }
            if matches!(command, ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyEditorOnly) {
                if let Some(syntax_theme) = config::set_editor_theme(&name) {
                    app.syntax_theme = Some(syntax_theme);
                }
            }
            app.mode = Mode::Browsing;
        }
        ThemeMenuCommand::Ignore => {}
    }

    Ok(())
}


/// Renders the F9 color-scheme picker popup: a list of theme names
/// found in the config dir, or a hint that none were found. Moved here
/// from `ui.rs` so this module owns state, key handling, and rendering
/// for its own popup.
pub fn draw_theme_menu(frame: &mut Frame, area: Rect, menu: &ThemeMenu, theme: &Theme) {
    let height = (menu.themes.len().max(1) as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(46, height, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Color scheme ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if menu.themes.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "No themes found — drop a Windows Terminal scheme .json",
            Style::default().fg(theme.text_dim),
        )));
        frame.render_widget(empty, rows[0]);
    } else {
        let items: Vec<ListItem> = menu
            .themes
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let style = if index == menu.selected {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(name.clone(), style)))
            })
            .collect();
        frame.render_widget(List::new(items), rows[0]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" apply both  ", Style::default().fg(theme.text_dim)),
        Span::styled("I", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("nterface  ", Style::default().fg(theme.text_dim)),
        Span::styled("E", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("ditor  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


#[cfg(test)]
mod tests {
    use super::*;

    fn menu_with(themes: Vec<&str>) -> ThemeMenu {
        ThemeMenu { themes: themes.into_iter().map(String::from).collect(), selected: 0 }
    }

    #[test]
    fn move_down_clamped_at_last_theme() {
        let mut menu = menu_with(vec!["a", "b"]);
        menu.move_down();
        assert_eq!(menu.selected, 1);
        menu.move_down();
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn move_up_clamped_at_first_theme() {
        let mut menu = menu_with(vec!["a", "b"]);
        menu.move_up();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn selected_theme_is_none_when_list_is_empty() {
        let menu = menu_with(vec![]);
        assert_eq!(menu.selected_theme(), None);
    }

    #[test]
    fn selected_theme_tracks_the_cursor() {
        let mut menu = menu_with(vec!["a", "b", "c"]);
        menu.move_down();
        assert_eq!(menu.selected_theme(), Some("b"));
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn enter_applies_both() {
        assert_eq!(resolve(key(KeyCode::Enter)), ThemeMenuCommand::ApplyBoth);
    }

    #[test]
    fn i_and_e_apply_one_side_each() {
        assert_eq!(resolve(key(KeyCode::Char('i'))), ThemeMenuCommand::ApplyInterfaceOnly);
        assert_eq!(resolve(key(KeyCode::Char('E'))), ThemeMenuCommand::ApplyEditorOnly);
    }

    #[test]
    fn esc_closes() {
        assert_eq!(resolve(key(KeyCode::Esc)), ThemeMenuCommand::Close);
    }

    #[test]
    fn unbound_key_is_ignored() {
        assert_eq!(resolve(key(KeyCode::Char('z'))), ThemeMenuCommand::Ignore);
    }
}
