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

use crate::app::{App, Mode, ShellMenu};
use crate::theme::Theme;
use crate::ui::centered_rect;


/// A shell the command line can run typed input through — Windows
/// Terminal-style "profile" (see `.claude/rules/litastum-theming.md`'s
/// sibling doc, `.claude/rules/litastum-stack.md`, for the reasoning:
/// only universally-preinstalled shells are built in, no PATH/registry
/// probing for Git Bash/WSL/pwsh yet).
#[derive(Debug, Clone)]
pub struct ShellProfile {
    /// Shown in the `Ctrl+P` picker and the command-line's right edge.
    pub name: String,
    pub program: String,
    /// Arguments before the typed command itself, e.g. `["/C"]` for
    /// `cmd`, or `["-NoLogo", "-Command"]` for PowerShell.
    pub args_prefix: Vec<String>,
}


impl ShellProfile {
    fn new(name: &str, program: &str, args_prefix: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            program: program.to_string(),
            args_prefix: args_prefix.iter().map(|arg| arg.to_string()).collect(),
        }
    }
}


/// The built-in profiles for this platform. Always non-empty; index
/// `0` is the default `App::active_shell` starts on.
pub fn builtin_profiles() -> Vec<ShellProfile> {
    profiles_for_platform()
}


#[cfg(windows)]
fn profiles_for_platform() -> Vec<ShellProfile> {
    vec![
        ShellProfile::new("Command Prompt", "cmd", &["/C"]),
        ShellProfile::new("PowerShell", "powershell", &["-NoLogo", "-Command"]),
    ]
}


#[cfg(not(windows))]
fn profiles_for_platform() -> Vec<ShellProfile> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    vec![ShellProfile::new("Shell", &shell, &["-c"])]
}


/// Key handling on the `Ctrl+P` shell picker: `Up`/`Down` to move,
/// `Enter` sets `app.active_shell` and closes, `Esc` cancels. Not
/// persisted to `config.json` — resets to the platform default each
/// run (see `.claude/rules/litastum-stack.md`). Moved here from
/// `main.rs` so this module owns both the profile list and the popup
/// that picks from it (`app::ShellMenu`, the "which row is
/// highlighted" state, still lives on `App` — it's simple enough not
/// to need its own file the way `MainMenu`/`ThemeMenu` do).
pub fn handle_shell_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ShellMenu(menu) = &mut app.mode else {
        return Ok(());
    };
    debug!(?key, selected = menu.selected, "shell menu key");

    match key.code {
        KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
        KeyCode::Down => {
            if menu.selected + 1 < app.shell_profiles.len() {
                menu.selected += 1;
            }
        }
        KeyCode::Enter => {
            app.active_shell = menu.selected;
            app.mode = Mode::Browsing;
        }
        KeyCode::Esc => app.mode = Mode::Browsing,
        _ => {}
    }

    Ok(())
}


/// Renders the `Ctrl+P` shell-profile picker popup. Moved here from
/// `ui.rs` so this module owns the profile list, key handling, and
/// rendering for its own popup.
pub fn draw_shell_menu(frame: &mut Frame, area: Rect, menu: &ShellMenu, profiles: &[ShellProfile], theme: &Theme) {
    let height = (profiles.len() as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(36, height, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Shell ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let style = if index == menu.selected {
                Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(profile.name.clone(), style)))
        })
        .collect();
    frame.render_widget(List::new(items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" select  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_is_never_empty() {
        assert!(!builtin_profiles().is_empty());
    }
}
