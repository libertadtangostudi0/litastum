use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::explorer::{MenuItemBody, UserMenuPromptState, UserMenuState};
use crate::text_field;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders `F2`'s user menu: the current (possibly nested) level's
/// items, a `\u{203a}` suffix hinting which ones descend into a
/// submenu, or a "No items" hint for an empty level -- a present-but-
/// empty menu file, or a submenu block someone wrote with nothing in
/// it (a *missing* file is handled one step earlier, before this ever
/// opens -- `explorer::command::open_user_menu` creates one and opens
/// the built-in editor on it instead).
pub fn draw_user_menu(frame: &mut Frame, area: Rect, menu: &UserMenuState, theme: &Theme, style: PopupStyle) {
    let level = menu.current_level();
    let extra = popup::chrome_extra_rows(style);
    let height = (level.items.len().max(1) as u16 + 4 + extra).clamp(6 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" User menu ")), 46, height);

    let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    if level.items.is_empty() {
        frame.render_widget(Line::from(Span::styled("No items", Style::default().fg(theme.text_dim))), rows[0]);
    } else {
        let items: Vec<ListItem> = level
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut label = String::new();
                if let Some(hotkey) = item.hotkey {
                    label.push(hotkey);
                    label.push_str(": ");
                }
                label.push_str(&item.title);
                if matches!(item.body, MenuItemBody::Submenu(_)) {
                    label.push_str("  \u{203a}");
                }
                let style = if index == level.selected {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();
        frame.render_widget(List::new(items), rows[0]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" select  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" back", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


/// Renders a selected user-menu item's own `!?Label?Default!` prompt --
/// one text field at a time (`text_field.rs`, same editing surface as
/// `confirm.rs`'s transfer-destination field), with the label and
/// (once there's more than one) a "current of total" count in the
/// popup's own title. Returns where the real terminal cursor should
/// sit, same mechanism as every other text-entry popup in this app.
pub fn draw_user_menu_prompt(frame: &mut Frame, area: Rect, prompt: &UserMenuPromptState, theme: &Theme, style: PopupStyle) -> Position {
    let (current, total) = prompt.progress();
    let title = if total > 1 {
        format!(" {} ({current}/{total}) ", prompt.current_label())
    } else {
        format!(" {} ", prompt.current_label())
    };

    let extra = popup::chrome_extra_rows(style);
    let height = 4 + extra;
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(title)), 50, height);

    let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1)]).split(inner);

    frame.render_widget(field_line(prompt, theme), rows[0]);

    let next_label = if current < total { " next  " } else { " run  " };
    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(next_label, Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);

    Position { x: rows[0].x + prompt.cursor as u16, y: rows[0].y }
}


/// Renders the field's value with its `Shift+Left`/`Right` selection
/// (if any) picked out, same highlight `confirm.rs::destination_line`
/// uses for the transfer-destination field -- no selection just renders
/// as plain text.
fn field_line(prompt: &UserMenuPromptState, theme: &Theme) -> Line<'static> {
    let Some(anchor) = prompt.selection_anchor else {
        return Line::from(Span::styled(prompt.value.clone(), Style::default().fg(theme.text)));
    };

    let (start, end) = text_field::selection_range(anchor, prompt.cursor);
    let chars: Vec<char> = prompt.value.chars().collect();
    let before: String = chars[..start].iter().collect();
    let selected: String = chars[start..end].iter().collect();
    let after: String = chars[end..].iter().collect();

    Line::from(vec![
        Span::styled(before, Style::default().fg(theme.text)),
        Span::styled(selected, Style::default().fg(theme.text).bg(theme.current_row_bg)),
        Span::styled(after, Style::default().fg(theme.text)),
    ])
}


#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn rendered_menu(menu: &UserMenuState, style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| draw_user_menu(frame, frame.area(), menu, &theme, style)).unwrap();
        buffer_text(terminal.backend().buffer())
    }

    fn menu_from(content: &str) -> UserMenuState {
        let dir = unique_scratch_dir("ui-user-menu");
        fs::write(dir.join("LitastumMenu.ini"), content).unwrap();
        UserMenuState::open(&dir).unwrap()
    }

    #[test]
    fn shows_items_and_a_submenu_marker() {
        let menu = menu_from("s: status\ngit status -s\n\np: parent\n{\nc: child\necho hi\n}\n");
        let text = rendered_menu(&menu, PopupStyle::Rounded);
        assert!(text.contains("status"));
        assert!(text.contains("parent"));
        assert!(text.contains('\u{203a}'), "submenu item should show the descend marker");
    }

    #[test]
    fn empty_level_shows_a_hint_instead_of_panicking() {
        let menu = menu_from("");
        let text = rendered_menu(&menu, PopupStyle::Rounded);
        assert!(text.contains("No items"));
    }

    #[test]
    fn classic_style_still_renders_without_panicking() {
        let menu = menu_from("s: status\ngit status -s\n");
        let text = rendered_menu(&menu, PopupStyle::Classic);
        assert!(text.contains("status"));
    }

    #[test]
    fn prompt_shows_the_label_and_places_the_cursor_after_the_value() {
        let prompts = vec![crate::explorer::Prompt { label: "Commit title".to_string(), default: String::new() }];
        let commands = vec!["git commit -m \"!?Commit title?!\"".to_string()];
        let prompt = UserMenuPromptState::new(commands, prompts);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut cursor = Position::default();
        terminal
            .draw(|frame| {
                cursor = draw_user_menu_prompt(frame, frame.area(), &prompt, &theme, PopupStyle::Rounded);
            })
            .unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Commit title"));
        assert!(cursor.x > 0);
    }
}
