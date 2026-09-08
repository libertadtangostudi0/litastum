use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding},
    Frame,
};

use crate::theming::Theme;
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

/// Draws a floating popup's shared chrome -- rounded border, no
/// background fill of its own (an active experiment -- see below), no
/// title baked into the border. Callers draw their own title/content/
/// footer inside the returned inner `Rect` (already shrunk by
/// `PADDING`, on top of the border itself -- callers must budget for
/// both when picking `width`/`height`), since the three popups this
/// was built for (F9 menu, color-scheme picker, delete confirm) each
/// lay that out differently enough (a subtitle line here, a badge
/// there, a toggle row only in one of them) that forcing one rigid
/// header/footer API onto all three would fit worse than just sharing
/// the frame itself.
///
/// Rounded corners (`BorderType::Rounded`) plus *any* explicit
/// background fill were tried repeatedly and each combination looked
/// worse than the last: the corner glyphs (`╭╮╰╯`) are just Unicode
/// line-drawing characters suggesting a curve, but the character
/// *cell* underneath is always a hard square, and no fill -- solid,
/// cut at the corners, or a diagonal quadrant-block chamfer -- can
/// clip itself to that curve the way a real CSS `border-radius` (the
/// reference mockup this was built from) can; a popup's corner is only
/// one character cell, nowhere near enough resolution for an actual
/// curve by any means a character grid offers. This version is the
/// other extreme: rounded corners with no fill *at all*, being
/// re-tried on request after plain square corners -- if this still
/// doesn't read as intended, the corner glyph itself (not the fill) is
/// the remaining variable to reconsider.
///
/// Replaces each popup's own former `Block::default().borders(Borders::ALL)`
/// with the title baked directly into the border line -- what
/// `ui/menu.rs`, `ui/confirm.rs`, `ui/theme_menu.rs` each drew
/// individually before sharing this instead.
pub fn draw_frame(frame: &mut Frame, area: Rect, theme: &Theme, width: u16, height: u16) -> Rect {
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .padding(PADDING);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    inner
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


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn render<F: FnOnce(&mut Frame)>(width: u16, height: u16, f: F) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| f(frame)).unwrap();
        terminal
    }

    #[test]
    fn draw_frame_does_not_panic_on_a_tiny_area() {
        let theme = Theme::dark();
        render(10, 5, |frame| {
            draw_frame(frame, frame.area(), &theme, 30, 10);
        });
    }

    #[test]
    fn draw_frame_returns_an_inner_rect_smaller_than_the_popup() {
        let theme = Theme::dark();
        let terminal = render(40, 20, |frame| {
            let inner = draw_frame(frame, frame.area(), &theme, 30, 10);
            assert!(inner.width < 30);
            assert!(inner.height < 10);
        });
        drop(terminal);
    }

    /// Real regression coverage for the "gap between the border and the
    /// content should look the same on every side" request: measures
    /// `inner`'s actual distance from the popup's own border on all
    /// four sides (independently computed via `centered_rect`, the
    /// same helper `draw_frame` itself uses) and asserts they're all
    /// equal cell counts -- not just that `PADDING`'s literal fields
    /// happen to match, in case a future change reintroduces the
    /// border/title-row special-casing that made `Block::inner` treat
    /// horizontal and vertical differently before.
    #[test]
    fn the_gap_between_border_and_content_is_equal_on_every_side() {
        let theme = Theme::dark();
        let area = Rect::new(0, 0, 60, 30);
        let popup = crate::ui::centered_rect(46, 15, area);

        let mut inner = Rect::default();
        render(60, 30, |frame| {
            inner = draw_frame(frame, frame.area(), &theme, 46, 15);
        });

        let left_gap = inner.left() - popup.left() - 1; // -1 for the border itself
        let top_gap = inner.top() - popup.top() - 1;
        let right_gap = (popup.right() - 1) - inner.right();
        let bottom_gap = (popup.bottom() - 1) - inner.bottom();

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
}
