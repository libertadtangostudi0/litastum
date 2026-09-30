use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding},
    Frame,
};

use crate::choice_menu::ChoiceMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::centered_rect;

/// Gap between a popup's border and its content: equal cell counts on
/// every side, not `ratatui`'s visually-equal `+2`/`+1`, which read as
/// uneven at the rounded corner. History: docs/history/popups.md.
const PADDING: Padding = Padding::uniform(2);

/// Rows of chrome `style` needs beyond `Classic`'s border: `Rounded`
/// adds padding top and bottom plus the title row. Popup heights add
/// this so `Rounded` keeps room for content.
pub fn chrome_extra_rows(style: PopupStyle) -> u16 {
    match style {
        PopupStyle::Classic => 0,
        PopupStyle::Rounded => 2 * PADDING.top + 1,
    }
}

/// Draws a popup's chrome and title and returns the content `Rect`, so
/// callers lay out content once for both styles (F9 -> Options -> UI).
///
/// - `Rounded`: rounded border, no fill of its own, uniform padding,
///   title as the first content line. No fill can follow a corner
///   glyph's curve in a character cell -- see
///   `.claude/rules/litastum-popup-design.md` before changing this.
/// - `Classic`: square border with the title on it, no padding -- kept
///   as a permanent alternative.
pub fn draw_frame(frame: &mut Frame, area: Rect, theme: &Theme, style: PopupStyle, title: Line<'static>, width: u16, height: u16) -> Rect {
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);

    match style {
        PopupStyle::Classic => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title(title);
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            inner
        }
        PopupStyle::Rounded => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.border))
                .padding(PADDING);
            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(inner);
            frame.render_widget(title, rows[0]);
            rows[1]
        }
    }
}


/// `percent` of `area`'s width, for a popup that should scale with the
/// terminal (Find file); `draw_frame` still clamps to `area`.
pub fn percent_width(area: Rect, percent: u16) -> u16 {
    ((area.width as u32 * percent as u32) / 100) as u16
}


/// A dim horizontal rule separating a popup's sections.
pub fn separator(width: u16, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width as usize), Style::default().fg(theme.border)))
}


/// One `key label` footer hint, styled as a filled pill (padded
/// background block) rather than plain colored text -- matching the
/// reference mockup's `y delete` / `esc keep` / `↵ open` footer style.
/// `bg` is the pill's fill color (`theme.danger` for a destructive
/// action, `theme.accent` for a neutral one); its text always renders
/// in `theme.bg` for contrast against either fill.
pub fn key_pill(key: &str, label: &str, bg: ratatui::style::Color, theme: &Theme) -> Span<'static> {
    Span::styled(format!(" {key} {label} "), Style::default().fg(theme.bg).bg(bg))
}


/// The selected row in any list popup: `current_row_bg`, bold, with the
/// scheme's `selection_text` when set (else `theme.text`). Every popup
/// goes through this so a bright selection background stays readable.
/// History: docs/history/popups.md.
pub fn selected_row_style(theme: &Theme) -> Style {
    Style::default().fg(theme.selection_text.unwrap_or(theme.text)).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
}

/// `selected_row_style` without bold, for a text selection inside a
/// field (Copy/Move destination, prompts, the command line).
pub fn selected_text_style(theme: &Theme) -> Style {
    Style::default().fg(theme.selection_text.unwrap_or(theme.text)).bg(theme.current_row_bg)
}


/// A single-column list popup: title, labels with the selected row
/// highlighted, and an `Enter`/`Esc` hint row. Callers format their own
/// labels (suffixes, columns); that part differs too much to share.
pub fn draw_list_popup(frame: &mut Frame, area: Rect, theme: &Theme, style: PopupStyle, title: &'static str, width: u16, labels: &[String], selected: usize, enter_label: &str, esc_label: &str) {
    let extra = chrome_extra_rows(style);
    let height = (labels.len().max(1) as u16 + 4 + extra).clamp(6 + extra, area.height);
    let inner = draw_frame(frame, area, theme, style, Line::from(Span::raw(title)), width, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let item_style = if index == selected { selected_row_style(theme) } else { Style::default().fg(theme.text) };
            ListItem::new(Line::from(Span::styled(label.clone(), item_style)))
        })
        .collect();
    frame.render_widget(List::new(items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {enter_label}  "), Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {esc_label}"), Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


/// A `ChoiceMenu`'s option labels for `draw_list_popup`, the option
/// equal to `current` (the setting actually in effect, not the
/// highlighted row) suffixed with " (current)".
pub fn choice_labels<T: Copy + PartialEq>(menu: &ChoiceMenu<T>, label: impl Fn(T) -> String, current: Option<T>) -> Vec<String> {
    menu.options()
        .iter()
        .map(|&option| if Some(option) == current { format!("{} (current)", label(option)) } else { label(option) })
        .collect()
}


#[cfg(test)]
mod tests {
    use crate::test_support::buffer_text;
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn rendered(labels: &[String], selected: usize, style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_list_popup(frame, frame.area(), &theme, style, " Test ", 30, labels, selected, "pick", "cancel");
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn draw_list_popup_shows_every_label_and_both_hints() {
        let labels = vec!["Alpha".to_string(), "Beta".to_string()];
        let text = rendered(&labels, 0, PopupStyle::Rounded);
        assert!(text.contains("Alpha"));
        assert!(text.contains("Beta"));
        assert!(text.contains("pick"));
        assert!(text.contains("cancel"));
    }

    #[test]
    fn draw_list_popup_highlights_the_selected_row() {
        let labels = vec!["Alpha".to_string(), "Beta".to_string()];
        let theme = Theme::dark();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_list_popup(frame, frame.area(), &theme, PopupStyle::Rounded, " Test ", 30, &labels, 1, "pick", "cancel");
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let selected_bg = selected_row_style(&theme).bg;
        let has_a_highlighted_row = (0..buffer.area.height).any(|y| (0..buffer.area.width).any(|x| buffer[(x, y)].bg == selected_bg.unwrap()));
        assert!(has_a_highlighted_row, "the row at index 1 (\"Beta\") should render with the selected-row background");
    }

    #[test]
    fn draw_list_popup_does_not_panic_on_an_empty_list() {
        let labels: Vec<String> = Vec::new();
        let text = rendered(&labels, 0, PopupStyle::Rounded);
        assert!(text.contains("pick"));
    }

    /// `selected_row_style`/`selected_text_style` both fall back to
    /// `theme.text` when a scheme sets no `selection_text` override --
    /// every scheme's behavior before that field existed, still the
    /// default for every scheme that doesn't ask for something else.
    #[test]
    fn selected_styles_fall_back_to_theme_text_without_an_override() {
        let theme = Theme::dark();
        assert_eq!(theme.selection_text, None);
        assert_eq!(selected_row_style(&theme).fg, Some(theme.text));
        assert_eq!(selected_text_style(&theme).fg, Some(theme.text));
    }

    /// The actual point of both helpers: a scheme that sets
    /// `selection_text` (requested directly, to keep text readable
    /// over a deliberately bright selection background) overrides it
    /// in both list rows and in-place text selections alike.
    #[test]
    fn selected_styles_use_the_theme_override_when_set() {
        let mut theme = Theme::dark();
        theme.selection_text = Some(ratatui::style::Color::Rgb(0, 0, 0));
        assert_eq!(selected_row_style(&theme).fg, Some(ratatui::style::Color::Rgb(0, 0, 0)));
        assert_eq!(selected_text_style(&theme).fg, Some(ratatui::style::Color::Rgb(0, 0, 0)));
    }

    fn render<F: FnOnce(&mut Frame)>(width: u16, height: u16, f: F) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| f(frame)).unwrap();
        terminal
    }

    fn plain_title(text: &str) -> Line<'static> {
        Line::from(Span::raw(text.to_string()))
    }

    #[test]
    fn draw_frame_does_not_panic_on_a_tiny_area() {
        let theme = Theme::dark();
        render(10, 5, |frame| {
            draw_frame(frame, frame.area(), &theme, PopupStyle::Rounded, plain_title("x"), 30, 10);
        });
    }

    #[test]
    fn draw_frame_returns_an_inner_rect_smaller_than_the_popup() {
        let theme = Theme::dark();
        let terminal = render(40, 20, |frame| {
            let inner = draw_frame(frame, frame.area(), &theme, PopupStyle::Rounded, plain_title("x"), 30, 10);
            assert!(inner.width < 30);
            assert!(inner.height < 10);
        });
        drop(terminal);
    }

    /// The padding gap measures the same on all four sides, from the
    /// real `Block::inner`, not `PADDING`'s fields. Measured before the
    /// title row, which `draw_frame` reserves on top by design.
    #[test]
    fn the_gap_between_border_and_content_is_equal_on_every_side() {
        let area = Rect::new(0, 0, 60, 30);
        let popup = crate::ui::centered_rect(46, 15, area);

        let padded = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).padding(PADDING).inner(popup);

        let left_gap = padded.left() - popup.left() - 1; // -1 for the border itself
        let top_gap = padded.top() - popup.top() - 1;
        let right_gap = (popup.right() - 1) - padded.right();
        let bottom_gap = (popup.bottom() - 1) - padded.bottom();

        assert_eq!(left_gap, top_gap, "left and top gaps should match");
        assert_eq!(left_gap, right_gap, "left and right gaps should match");
        assert_eq!(left_gap, bottom_gap, "left and bottom gaps should match");
    }

    #[test]
    fn separator_repeats_the_line_character_to_the_requested_width() {
        let line = separator(12, &Theme::dark());
        assert_eq!(line.width(), 12);
    }

    #[test]
    fn key_pill_pads_the_key_and_label_with_spaces() {
        let pill = key_pill("y", "delete", ratatui::style::Color::Red, &Theme::dark());
        assert_eq!(pill.content, " y delete ");
    }


    /// `Classic` bakes the title into the border line itself -- no
    /// separate content row consumed for it, unlike `Rounded` below.
    #[test]
    fn classic_style_bakes_the_title_into_the_border() {
        let theme = Theme::dark();
        let terminal = render(40, 20, |frame| {
            draw_frame(frame, frame.area(), &theme, PopupStyle::Classic, plain_title("My Title"), 30, 10);
        });
        let text = buffer_text(terminal.backend().buffer());
        let border_line = text.lines().find(|line| line.contains('┌')).expect("should have a top border line");
        assert!(border_line.contains("My Title"), "title should sit on the border's own top line: {border_line:?}");
    }

    /// `Rounded` draws the title as its own first content line, inside
    /// the border and padding -- the returned content rect starts one
    /// row below it.
    #[test]
    fn rounded_style_draws_the_title_as_its_own_content_line() {
        let theme = Theme::dark();
        let terminal = render(40, 20, |frame| {
            draw_frame(frame, frame.area(), &theme, PopupStyle::Rounded, plain_title("My Title"), 30, 10);
        });
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("My Title"), "title should render somewhere inside the padded frame: {text}");
    }
}
