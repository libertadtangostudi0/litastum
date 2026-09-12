use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::app::{DeleteEntry, PendingDelete, PendingTransfer, TransferOp};
use crate::text_field;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the F8 "delete this?" prompt over the browser -- a small
/// card (`popup::draw_frame`) rather than the plain bordered box this
/// used before: a danger-dot + "Delete" title with an "F8" badge at
/// the right edge, the entry's name and (for a single file -- see
/// `DeleteEntry::size`'s own doc comment) its size and path, then a
/// separator and pill-style `y delete` / `esc keep` hints. Redesigned
/// from a reference mockup reviewed alongside the F9 menu and
/// color-scheme-picker redesigns (`ui/menu.rs`, `ui/theme_menu.rs`).
///
/// Several entries (Far Manager-style multi-select, see
/// `PendingDelete::entries`'s own doc comment) are summarized by count
/// instead of named individually -- there's no room to list every name
/// on one line, matching how the F5/F6 transfer prompt handles the same
/// situation (`draw_confirm_transfer_popup`).
pub fn draw_confirm_delete_popup(frame: &mut Frame, area: Rect, pending: &PendingDelete, theme: &Theme, style: PopupStyle) {
    const WIDTH: u16 = 50;
    // 4 content rows either way, plus whatever chrome each style adds
    // around them: `Classic` just the 2 border rows; `Rounded` the 2
    // border rows, its own uniform padding (2 rows top+bottom), and the
    // title's own content row.
    let height = match style {
        PopupStyle::Classic => 4 + 2,
        PopupStyle::Rounded => 4 + 1 + 2 * (1 + 2),
    };

    // The danger-dot + bold "Delete" + right-aligned "F8" badge only
    // reads well with `Rounded`'s own padded content line -- `Classic`
    // bakes whatever title it's given straight into the border instead
    // (no room for a right-aligned badge there), so it gets a plain
    // flat string, same as every other Classic-style popup.
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Delete (F8) ")),
        PopupStyle::Rounded => {
            // Content width once `Rounded`'s border + padding are
            // subtracted -- see `popup.rs`'s own `PADDING` constant (2
            // cells, each side) plus the 1-cell border itself.
            let content_width = WIDTH.saturating_sub(2 * (1 + 2));
            Line::from(vec![
                Span::styled("● ", Style::default().fg(theme.danger)),
                Span::styled("Delete", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::raw(" ".repeat((content_width as usize).saturating_sub(11))),
                Span::styled("F8", Style::default().fg(theme.text_dim)),
            ])
        }
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, WIDTH, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let subject = match pending.entries.as_slice() {
        [only] => only.name.clone(),
        several => format!("{} items", several.len()),
    };
    frame.render_widget(Line::from(Span::styled(subject, Style::default().fg(theme.text))), rows[0]);

    let info = match pending.entries.as_slice() {
        [only] if only.is_dir => format!("directory · {}", only.path.display()),
        [only] => format!("{} · {}", format_size(only.size), only.path.display()),
        several => several
            .first()
            .and_then(|entry: &DeleteEntry| entry.path.parent())
            .map(|parent| parent.display().to_string())
            .unwrap_or_default(),
    };
    frame.render_widget(Line::from(Span::styled(info, Style::default().fg(theme.text_dim))), rows[1]);

    frame.render_widget(popup::separator(inner.width, theme), rows[2]);

    let hints = Line::from(vec![popup::key_pill("y", "delete", theme.danger, theme), Span::raw("  "), popup::key_pill("esc", "keep", theme.accent, theme)]);
    frame.render_widget(hints, rows[3]);
}


/// Human-readable byte count for the delete popup's file-size line
/// (`"23.3 kB"`-style, metric units matching the reference mockup --
/// not `drive_menu.rs::format_bytes`'s GiB/MiB, which only makes sense
/// for the much larger free-space figures it's built for and would
/// round a typical file down to `"0.0 M"`).
fn format_size(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    const MB: f64 = KB * 1000.0;
    const GB: f64 = MB * 1000.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} kB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Renders the F5/F6 "copy/move to?" prompt: source name and operation
/// on one line, an editable destination path on the next (defaults to
/// the other panel's directory — see `explorer::command::request_transfer`).
/// Returns where the real terminal cursor should sit, same mechanism as
/// the command line's own cursor (`ui::draw`).
pub fn draw_confirm_transfer_popup(frame: &mut Frame, area: Rect, pending: &PendingTransfer, theme: &Theme, style: PopupStyle) -> Position {
    let verb = match pending.operation {
        TransferOp::Copy => "Copy",
        TransferOp::Move => "Move",
    };
    // 4 content rows either way -- see `draw_confirm_delete_popup`'s own
    // comment for why the two styles need different total heights to
    // both land on the same content-row count.
    let height = match style {
        PopupStyle::Classic => 4 + 2,
        PopupStyle::Rounded => 4 + 1 + 2 * (1 + 2),
    };
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(format!(" {verb} "))), 60, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    // A single source names it directly, matching the prompt's
    // original wording; several (Far Manager-style multi-select, see
    // `PendingTransfer::sources`'s own doc comment) are summarized by
    // count instead -- there's no room to list every name on one line,
    // and Far's own equivalent prompt does the same.
    let subject = match pending.sources.as_slice() {
        [only] => format!("'{}'", only.name),
        several => format!("{} items", several.len()),
    };
    let source_line = Line::from(Span::styled(
        format!("{verb} {subject} to:"),
        Style::default().fg(theme.text),
    ));
    frame.render_widget(source_line, rows[0]);

    frame.render_widget(destination_line(pending, theme), rows[1]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {verb}   "), Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[3]);

    Position {
        x: rows[1].x + pending.cursor as u16,
        y: rows[1].y,
    }
}

/// Renders the destination text with its `Shift+Left`/`Shift+Right`
/// selection (`text_field::selection_range`), if any, picked out with
/// the same highlight background used for the active row in a panel
/// (`theme.current_row_bg`) — no selection just renders as plain text.
fn destination_line(pending: &PendingTransfer, theme: &Theme) -> Line<'static> {
    let Some(anchor) = pending.selection_anchor else {
        return Line::from(Span::styled(pending.destination.clone(), Style::default().fg(theme.text)));
    };

    let (start, end) = text_field::selection_range(anchor, pending.cursor);
    let chars: Vec<char> = pending.destination.chars().collect();
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
    use std::path::PathBuf;

    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    #[test]
    fn format_size_picks_the_right_unit() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(23_300), "23.3 kB");
        assert_eq!(format_size(4_200_000), "4.2 MB");
        assert_eq!(format_size(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn draw_confirm_delete_popup_does_not_panic_and_shows_the_size() {
        let pending = PendingDelete {
            entries: vec![DeleteEntry { path: PathBuf::from("C:/dev/litastum/TODO.md"), name: "TODO.md".into(), is_dir: false, size: 23_300 }],
        };
        let theme = Theme::dark();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_confirm_delete_popup(frame, frame.area(), &pending, &theme, PopupStyle::Rounded);
            })
            .unwrap();

        let contents = buffer_text(terminal.backend().buffer());
        assert!(contents.contains("Delete"));
        assert!(contents.contains("TODO.md"));
        assert!(contents.contains("23.3 kB"));
        assert!(contents.contains("delete"));
        assert!(contents.contains("keep"));
    }

    #[test]
    fn draw_confirm_delete_popup_classic_style_still_shows_the_size() {
        let pending = PendingDelete {
            entries: vec![DeleteEntry { path: PathBuf::from("C:/dev/litastum/TODO.md"), name: "TODO.md".into(), is_dir: false, size: 23_300 }],
        };
        let theme = Theme::dark();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_confirm_delete_popup(frame, frame.area(), &pending, &theme, PopupStyle::Classic);
            })
            .unwrap();

        let contents = buffer_text(terminal.backend().buffer());
        assert!(contents.contains("Delete"));
        assert!(contents.contains("TODO.md"));
        assert!(contents.contains("23.3 kB"));
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
