use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding},
    Frame,
};

use crate::theming::{PopupStyle, Theme};
use crate::ui::centered_rect;

/// Horizontal and vertical breathing room between a popup's border and
/// its content -- content used to start flush against the border on
/// every side (`Block::inner` alone), reported as visually cramped
/// against the reference mockup, which has real margin on all four
/// sides.
///
/// Equal cell counts on all four sides, not the `+2` horizontal / `+1`
/// vertical `ratatui::widgets::Padding` itself recommends for visually
/// *equal-looking* padding (terminal cells are roughly twice as tall
/// as they are wide, so doubling the horizontal count compensates) --
/// tried first, but reported uneven specifically at the rounded
/// corner: with a rounded border, the gap between the curve and the
/// content needs to actually match cell-for-cell in both directions
/// for the corner itself to read as symmetric, which the
/// visually-equal-but-numerically-different padding doesn't give.
const PADDING: Padding = Padding::uniform(2);

/// How many *extra* rows of chrome `style` needs beyond `Classic`'s own
/// (border top + bottom, title free -- baked into the border line) --
/// for `Rounded`, that's `PADDING`'s own 2 rows top + 2 bottom, plus the
/// one content row `draw_frame` reserves for the title itself.
/// `draw_list_popup` sizes its own popup height off of this, so a
/// height formula tuned for `Classic`'s tighter chrome doesn't leave
/// `Rounded` with zero or negative room for its own content once the
/// border/padding/title are subtracted.
pub fn chrome_extra_rows(style: PopupStyle) -> u16 {
    match style {
        PopupStyle::Classic => 0,
        PopupStyle::Rounded => 2 * PADDING.top + 1,
    }
}

/// Draws a floating popup's shared chrome and its title, returning the
/// remaining content `Rect` for the caller's own list/hints/whatever
/// else -- uniform across both `PopupStyle`s, so a caller lays out its
/// content rows exactly once regardless of which style is active
/// (F9 -> Options -> UI). `title` accepts a full `Line` (not just a
/// plain string) so callers that want more than flat text -- a badge, a
/// scroll-arrow, a leading dot -- can still build one; a plain
/// `Line::from(Span::raw(...))` works for everything simpler.
///
/// - `PopupStyle::Rounded`: rounded border, no background fill of its
///   own (an active experiment -- see below), uniform padding, `title`
///   drawn as the frame's own first content line. Corner glyphs
///   (`╭╮╰╯`) plus *any* explicit background fill were tried
///   repeatedly and each combination looked worse than the last: the
///   glyphs are just Unicode line-drawing characters suggesting a
///   curve, but the character *cell* underneath is always a hard
///   square, and no fill -- solid, cut at the corners, or a diagonal
///   quadrant-block chamfer -- can clip itself to that curve the way a
///   real CSS `border-radius` (the reference mockup this was built
///   from) can; a popup's corner is only one character cell, nowhere
///   near enough resolution for an actual curve by any means a
///   character grid offers. Landed on rounded corners with no fill at
///   all -- see `.claude/rules/litastum-popup-design.md` for the full
///   back-and-forth; don't re-litigate this without a genuinely new
///   idea, not a variation already listed there.
/// - `PopupStyle::Classic`: plain square `Block::borders(ALL)`, `title`
///   baked directly into the border line, no interior padding -- the
///   look every popup in this app used before this module existed,
///   requested back as a permanent, coexisting alternative rather than
///   something to migrate away from entirely (F9 -> Options -> UI).
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


/// A popup width that's `percent` of `area`'s own width -- `draw_frame`
/// itself still clamps to `area.width` as a hard ceiling, so this is
/// purely for popups that should visibly scale with the real terminal
/// window rather than sit at one fixed cell count regardless of how
/// wide the app actually is. Added for Find file's own popup (both the
/// typing form and its results list): reported directly as too narrow
/// on a wide terminal, compared against most of this app's other
/// popups, which stay a fixed width by design (a short list/prompt
/// genuinely doesn't need to grow with the window).
pub fn percent_width(area: Rect, percent: u16) -> u16 {
    ((area.width as u32 * percent as u32) / 100) as u16
}


/// A dim horizontal rule the width of `area`, separating a popup's
/// header/content/footer sections -- every popup in the reference
/// mockup has one before its footer hint row.
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


/// Style for a selected row in a list-style popup (F9 menu, the theme/
/// shell/drive/popup-style pickers, Find file's own results, the
/// Markdown link search list, the command-history popup) --
/// `theme.current_row_bg` background, bold, with `theme.selection_text`
/// overriding the text color when a scheme actually sets it (falls back
/// to `theme.text`, every scheme's behavior before that field existed).
/// Pulled out once identical `Style::default().fg(theme.text).bg(theme.current_row_bg)
/// .add_modifier(Modifier::BOLD)` literals had spread to essentially
/// every popup's own list rendering -- reported directly after a scheme
/// set `current_row_bg` to a bright, saturated color (a vivid ANSI
/// green) specifically so its own selected file-panel row would read
/// black-on-green: every *other* popup's selected row kept using
/// `theme.text` unconditionally and read poorly the same way the panel
/// row used to, since none of them knew about the override yet.
pub fn selected_row_style(theme: &Theme) -> Style {
    Style::default().fg(theme.selection_text.unwrap_or(theme.text)).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
}

/// Same background/foreground pairing as `selected_row_style`, for an
/// in-place text *selection* within an editable field (the Copy/Move
/// destination field, a user-menu prompt field, the always-live command
/// line's own `Shift`+arrow selection) -- no bold, since this highlights
/// a run of characters within running text, not a whole list row.
pub fn selected_text_style(theme: &Theme) -> Style {
    Style::default().fg(theme.selection_text.unwrap_or(theme.text)).bg(theme.current_row_bg)
}


/// Renders a plain, single-column list popup: title, a `List` with the
/// highlighted row picked out, and an `Enter <label>  Esc <label>`
/// footer hint -- the shape shared by `ui/menu.rs`, `ui/shell.rs`,
/// `ui/drive_menu.rs`, `ui/popup_style_menu.rs`, and the built-in
/// editor's own `ui/editor_menu.rs`/`ui/editor_keymap_menu.rs`, pulled
/// out once six call sites had all copied the same ~25 lines with only
/// the title, width, item labels, and the two hint descriptions
/// actually differing. Each caller formats its own `labels` first (a
/// bare `&str`, a `"(current)"` suffix, `drive_menu.rs`'s own
/// multi-column layout, ...) rather than this function taking raw
/// domain data -- what varies there differs enough per caller that
/// trying to generalize it here would have meant more parameters than
/// the duplication it replaces was worth.
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


#[cfg(test)]
mod tests {
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
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
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

    /// Real regression coverage for the "gap between the border and the
    /// content should look the same on every side" request: measures
    /// the frame's own actual distance from the popup's border on all
    /// four sides (independently computed via `centered_rect`, the
    /// same helper `draw_frame` itself uses) and asserts they're all
    /// equal cell counts -- not just that `PADDING`'s literal fields
    /// happen to match, in case a future change reintroduces the
    /// border/title-row special-casing that made `Block::inner` treat
    /// horizontal and vertical differently before.
    ///
    /// Measured against the padded area *before* the title row is
    /// consumed (`Block::inner`, not `draw_frame`'s own returned
    /// content rect) -- `draw_frame` always reserves one more row at
    /// the top for the title itself, which is expected and unrelated to
    /// whether the border's own padding is symmetric.
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

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
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
