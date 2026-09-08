use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theming::{Theme, ThemeMenu, ThemeMenuEntry};
use crate::ui::popup;

/// Renders the F9 color-scheme picker popup: a list of theme names
/// found in the config dir (each with a 4-color swatch preview and,
/// if it matches what's actually configured, a "current" label), or a
/// hint that none were found. Redesigned onto the shared popup card
/// (`ui/popup.rs`) alongside the F9 menu and delete-confirm popups.
pub fn draw_theme_menu(frame: &mut Frame, area: Rect, menu: &ThemeMenu, theme: &Theme) {
    let height = (menu.themes.len().max(1) as u16 + 13).clamp(15, area.height);
    let inner = popup::draw_frame(frame, area, theme, 46, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // legend
            Constraint::Length(1), // blank -- breathing room before the list, proportional to the gap after it
            Constraint::Min(1),    // list
            Constraint::Length(1), // blank -- same gap, before the footer separator
            Constraint::Length(1), // separator
            Constraint::Length(1), // footer hint
        ])
        .split(inner);

    let needs_scroll = menu.themes.len() as u16 + 13 > height;
    let title = if needs_scroll {
        Line::from(vec![
            Span::styled("Color scheme", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            Span::raw(" ".repeat((inner.width as usize).saturating_sub(13))),
            Span::styled("⌃", Style::default().fg(theme.text_dim)),
        ])
    } else {
        Line::from(Span::styled("Color scheme", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)))
    };
    frame.render_widget(title, rows[0]);

    frame.render_widget(popup::separator(inner.width, theme), rows[1]);

    let legend = Line::from(vec![
        Span::styled("• ", Style::default().fg(theme.text_dim)),
        Span::styled("interface", Style::default().fg(theme.text)),
        Span::raw("   "),
        Span::styled("• ", Style::default().fg(theme.text_dim)),
        Span::styled("editor", Style::default().fg(theme.text)),
    ]);
    frame.render_widget(legend, rows[2]);

    if menu.themes.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "No themes found — drop a Windows Terminal scheme .json",
            Style::default().fg(theme.text_dim),
        )));
        frame.render_widget(empty, rows[4]);
    } else {
        let items: Vec<ListItem> = menu
            .themes
            .iter()
            .enumerate()
            .map(|(i, entry)| theme_row(entry, menu, theme, inner.width, i == menu.selected))
            .collect();
        // No `.highlight_style()` here -- `List` applies that to the
        // *entire* row_area unconditionally (see
        // `ratatui-widgets::list::rendering`), which is exactly the
        // full-bleed bar `theme_row` deliberately avoids below. Manual
        // per-row background instead; `ListState` is still needed so
        // `List` scrolls the highlighted row into view once the list
        // overflows.
        let list = List::new(items);
        let mut state = ListState::default().with_selected(Some(menu.selected));
        frame.render_stateful_widget(list, rows[4], &mut state);
    }

    frame.render_widget(popup::separator(inner.width, theme), rows[6]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" apply  ", Style::default().fg(theme.text_dim)),
        Span::styled("i", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" interface  ", Style::default().fg(theme.text_dim)),
        Span::styled("e", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" editor", Style::default().fg(theme.text_dim)),
    ])
    .centered();
    frame.render_widget(hint, rows[7]);
}


/// One theme's row: a 4-dot swatch (or a dash if the file failed to
/// load), its name, and -- right-aligned to `width`, matching the
/// reference mockup rather than sitting right next to the name -- a
/// "current" label if it's what's actually configured for either half
/// right now.
///
/// The selected row's highlight is a manually-painted background
/// rather than `List`'s own `highlight_style` (see the call site's own
/// comment for why), inset by one unhighlighted column on both the
/// left and right -- approximating the reference mockup's rounded
/// "pill" selector, whose curved ends recede slightly from the row's
/// true edges (an actual curve isn't renderable in a character grid,
/// so a 1-column gap stands in for it). Every row, selected or not,
/// gets that same leading/trailing column reserved -- otherwise
/// content would visibly shift sideways by one column the moment a
/// row becomes selected.
fn theme_row<'a>(entry: &'a ThemeMenuEntry, menu: &ThemeMenu, theme: &Theme, width: u16, is_selected: bool) -> ListItem<'a> {
    let highlight = is_selected.then_some(theme.border);
    let fg_style = |fg: ratatui::style::Color| {
        let style = Style::default().fg(fg);
        match highlight {
            Some(bg) => style.bg(bg),
            None => style,
        }
    };
    let filler_style = || match highlight {
        Some(bg) => Style::default().bg(bg),
        None => Style::default(),
    };

    let mut spans = vec![Span::raw(" ")]; // left margin -- see doc comment above
    let mut used = 1usize;

    match entry.swatch {
        Some(colors) => {
            for color in colors {
                spans.push(Span::styled("●", fg_style(color)));
                used += 1;
            }
        }
        None => {
            spans.push(Span::styled("-", fg_style(theme.text_dim)));
            used += 1;
        }
    }
    spans.push(Span::styled(" ", filler_style()));
    used += 1;
    spans.push(Span::styled(entry.name.as_str(), fg_style(theme.text)));
    used += entry.name.chars().count();

    let is_current = Some(entry.name.as_str()) == menu.current_interface.as_deref() || Some(entry.name.as_str()) == menu.current_editor.as_deref();
    if is_current {
        let label = "current";
        let gap = (width as usize).saturating_sub(1 + used + label.len()); // reserve the trailing margin column too
        spans.push(Span::styled(" ".repeat(gap.max(1)), filler_style()));
        spans.push(Span::styled(label, fg_style(theme.text_dim)));
    } else if highlight.is_some() {
        let gap = (width as usize).saturating_sub(1 + used);
        spans.push(Span::styled(" ".repeat(gap), filler_style()));
    }
    spans.push(Span::raw(" ")); // right margin -- see doc comment above

    ListItem::new(Line::from(spans))
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn menu_with(names: &[&str], current_interface: Option<&str>) -> ThemeMenu {
        ThemeMenu {
            themes: names
                .iter()
                .map(|name| ThemeMenuEntry { name: name.to_string(), swatch: Some([ratatui::style::Color::Red; 4]) })
                .collect(),
            selected: 0,
            current_interface: current_interface.map(String::from),
            current_editor: None,
        }
    }

    fn rendered(menu: &ThemeMenu) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_theme_menu(frame, frame.area(), menu, &theme);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_the_swatch_and_name_for_each_theme() {
        let menu = menu_with(&["dracula", "molocai"], None);
        let text = rendered(&menu);
        assert!(text.contains("●●●●"));
        assert!(text.contains("dracula"));
        assert!(text.contains("molocai"));
    }

    #[test]
    fn marks_the_currently_configured_theme() {
        let menu = menu_with(&["dracula", "molocai"], Some("dracula"));
        let text = rendered(&menu);
        let dracula_line = text.lines().find(|line| line.contains("dracula")).unwrap();
        // "current" should sit at the popup's own right edge (just
        // inside its padding/border), not right next to the name --
        // trim off the border and its padding, then check what's left
        // ends with "current". A plain `ends_with` on the raw line
        // would pass regardless of placement, since the row also
        // includes empty space outside the popup itself.
        let between_borders = dracula_line.split('│').nth(1).unwrap();
        assert!(between_borders.trim_end().ends_with("current"), "current should sit at the popup's right edge: {dracula_line:?}");
        assert!(!text.lines().any(|line| line.contains("molocai") && line.contains("current")));
    }

    /// The selected row's highlight is inset by one unstyled column on
    /// each side (the "pill" approximation, see `theme_row`'s own doc
    /// comment) rather than a full-bleed bar -- checks the actual
    /// rendered cell backgrounds, not just that some highlight exists,
    /// since a full-bleed bar would also make every assertion about
    /// "some cell is highlighted" pass.
    #[test]
    fn the_selected_rows_highlight_is_inset_by_one_column_on_each_side() {
        let menu = menu_with(&["dracula", "molocai"], None);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_theme_menu(frame, frame.area(), &menu, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        // Look for "dracula" as a whole word, not just the letter 'd'
        // -- the legend row's own "editor" also contains a 'd' and
        // renders *before* the list, so a single-letter search picked
        // that row instead of the actual (highlighted) list row.
        let row_text = |y: u16| -> String { (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect() };
        let y = (0..buffer.area.height).find(|&y| row_text(y).contains("dracula")).unwrap();

        // Find the popup's own left/right border columns on this row.
        // Content starts 1 (border) + 2 (`popup::PADDING`'s horizontal
        // padding) columns in from there -- that's `theme_row`'s own
        // leading margin space, then the swatch itself.
        let border_xs: Vec<u16> = (0..buffer.area.width).filter(|&x| buffer[(x, y)].symbol() == "│").collect();
        let (left_border, right_border) = (border_xs[0], border_xs[1]);

        // No background is painted behind unhighlighted cells at all
        // right now (see `popup::draw_frame`'s own doc comment -- an
        // active experiment) -- `Clear` leaves them at `Color::Reset`.
        let unhighlighted = ratatui::style::Color::Reset;
        assert_eq!(buffer[(left_border + 3, y)].bg, unhighlighted, "the row's own leading margin column should not be highlighted");
        assert_eq!(buffer[(left_border + 4, y)].bg, theme.border, "the swatch itself should be highlighted");
        assert_eq!(buffer[(right_border - 3, y)].bg, unhighlighted, "the row's own trailing margin column should not be highlighted");
    }

    #[test]
    fn shows_a_scroll_indicator_only_when_the_list_overflows_the_popup() {
        let few = menu_with(&["a", "b"], None);
        assert!(!rendered(&few).contains('⌃'), "a short list fitting entirely shouldn't show a scroll hint");

        let names: Vec<&str> = (0..30).map(|_| "x").collect();
        let many = menu_with(&names, None);
        assert!(rendered(&many).contains('⌃'), "a list taller than the popup's clamped height should hint that it scrolls");
    }

    #[test]
    fn empty_theme_list_shows_a_hint_instead_of_panicking() {
        let menu = menu_with(&[], None);
        let text = rendered(&menu);
        assert!(text.contains("No themes found"));
    }
}
