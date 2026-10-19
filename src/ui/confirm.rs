use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::app::{DeleteEntry, PendingCommand, PendingDelete, PendingTransfer, TransferOp};
use crate::explorer::PendingLink;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// The F8 delete prompt: a `popup::draw_frame` card with a danger dot,
/// "Delete" and an "F8" badge, the entry's name (plus size and path for a
/// single file), then `y delete` / `esc keep` pills. Several entries are
/// summarized by count, as in the transfer prompt. History: docs/history/popups.md.
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
    let (heading, badge, verb) = if pending.permanent { ("Delete for good", "Shift+Del", "delete") } else { ("To the Recycle Bin", "F8", "recycle") };
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(format!(" {heading} ({badge}) "))),
        PopupStyle::Rounded => {
            // Content width once `Rounded`'s border + padding are
            // subtracted -- see `popup.rs`'s own `PADDING` constant (2
            // cells, each side) plus the 1-cell border itself.
            let content_width = WIDTH.saturating_sub(2 * (1 + 2));
            let used = 2 + heading.chars().count() + badge.chars().count();
            Line::from(vec![
                Span::styled("● ", Style::default().fg(theme.danger)),
                Span::styled(heading, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::raw(" ".repeat((content_width as usize).saturating_sub(used))),
                Span::styled(badge, Style::default().fg(theme.text_dim)),
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

    let hints = Line::from(vec![popup::key_pill("y", verb, theme.danger, theme), Span::raw("  "), popup::key_pill("esc", "keep", theme.accent, theme)]);
    frame.render_widget(hints, rows[3]);
}


/// The prompt before a dangerous command (`Overlay::ConfirmCommand`): the
/// command, what it does, `y run` / `esc keep` -- the delete prompt's
/// shape.
pub fn draw_confirm_command_popup(frame: &mut Frame, area: Rect, pending: &PendingCommand, theme: &Theme, style: PopupStyle) {
    const WIDTH: u16 = 60;
    let height = match style {
        PopupStyle::Classic => 4 + 2,
        PopupStyle::Rounded => 4 + 1 + 2 * (1 + 2),
    };
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Run this command? ")),
        PopupStyle::Rounded => Line::from(vec![
            Span::styled("● ", Style::default().fg(theme.danger)),
            Span::styled("Run this command?", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        ]),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, WIDTH, height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(Line::from(Span::styled(pending.line.clone(), Style::default().fg(theme.text))), rows[0]);
    frame.render_widget(Line::from(Span::styled(format!("It {}.", pending.reason), Style::default().fg(theme.text_dim))), rows[1]);
    frame.render_widget(popup::separator(inner.width, theme), rows[2]);
    let hints = Line::from(vec![popup::key_pill("y", "run", theme.danger, theme), Span::raw("  "), popup::key_pill("esc", "keep", theme.accent, theme)]);
    frame.render_widget(hints, rows[3]);
}


/// The question before a save writes over a file changed on disk
/// (`Overlay::ConfirmOverwrite`): the file, `y overwrite` / `esc keep
/// editing` -- the delete prompt's shape.
pub fn draw_confirm_overwrite_popup(frame: &mut Frame, area: Rect, path: &std::path::Path, theme: &Theme, style: PopupStyle) {
    const WIDTH: u16 = 60;
    let height = match style {
        PopupStyle::Classic => 4 + 2,
        PopupStyle::Rounded => 4 + 1 + 2 * (1 + 2),
    };
    let heading = "Changed on disk";
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(format!(" {heading} "))),
        PopupStyle::Rounded => Line::from(vec![
            Span::styled("● ", Style::default().fg(theme.warning)),
            Span::styled(heading, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        ]),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, WIDTH, height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    frame.render_widget(Line::from(Span::styled(name, Style::default().fg(theme.text))), rows[0]);
    frame.render_widget(Line::from(Span::styled("changed since you opened it. Write yours over it?", Style::default().fg(theme.text_dim))), rows[1]);
    frame.render_widget(popup::separator(inner.width, theme), rows[2]);
    let hints = Line::from(vec![popup::key_pill("y", "overwrite", theme.danger, theme), Span::raw("  "), popup::key_pill("esc", "keep editing", theme.accent, theme)]);
    frame.render_widget(hints, rows[3]);
}


/// The question before an untrusted Markdown link opens
/// (`Overlay::ConfirmOpenLink`): its text, then where it really goes
/// (wrapped over two rows -- the start, its scheme, matters most), then
/// `y open` / `t always trust` / `esc cancel`.
pub fn draw_confirm_open_link_popup(frame: &mut Frame, area: Rect, pending: &PendingLink, theme: &Theme, style: PopupStyle) {
    const WIDTH: u16 = 70;
    const ROWS: u16 = 5;
    let height = match style {
        PopupStyle::Classic => ROWS + 2,
        PopupStyle::Rounded => ROWS + 1 + 2 * (1 + 2),
    };
    let heading = "Open this link?";
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(format!(" {heading} "))),
        PopupStyle::Rounded => Line::from(vec![
            Span::styled("● ", Style::default().fg(theme.warning)),
            Span::styled(heading, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        ]),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, WIDTH, height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(2), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(Line::from(Span::styled(pending.label().to_string(), Style::default().fg(theme.text_dim))), rows[0]);
    let destination = Paragraph::new(Span::styled(pending.destination(), Style::default().fg(theme.text))).wrap(Wrap { trim: false });
    frame.render_widget(destination, rows[1]);
    frame.render_widget(popup::separator(inner.width, theme), rows[2]);
    let hints = Line::from(vec![
        popup::key_pill("y", "open", theme.accent, theme),
        Span::raw("  "),
        popup::key_pill("t", "always trust", theme.warning, theme),
        Span::raw("  "),
        popup::key_pill("esc", "cancel", theme.accent, theme),
    ]);
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

    frame.render_widget(super::text_field::field_line(&pending.destination, theme), rows[1]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {verb}   "), Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[3]);

    Position {
        x: rows[1].x + pending.destination.cursor() as u16,
        y: rows[1].y,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::buffer_text;
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
            permanent: false,
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
        assert!(contents.contains("To the Recycle Bin"));
        assert!(contents.contains("TODO.md"));
        assert!(contents.contains("23.3 kB"));
        assert!(contents.contains("recycle"));
        assert!(contents.contains("keep"));
    }

    /// A link's text can say anything: the question shows where it goes.
    #[test]
    fn the_link_question_shows_where_the_link_really_goes() {
        let pending = PendingLink::url("Docs", "ms-settings:privacy");
        let theme = Theme::dark();
        for style in [PopupStyle::Rounded, PopupStyle::Classic] {
            let contents = crate::test_support::render_text(80, 24, |frame| draw_confirm_open_link_popup(frame, frame.area(), &pending, &theme, style));
            assert!(contents.contains("Open this link?"), "{style:?}");
            assert!(contents.contains("Docs"), "{style:?}");
            assert!(contents.contains("ms-settings:privacy"), "{style:?}");
            assert!(contents.contains("always trust"), "{style:?}");
        }
    }

    /// `Shift+Del` says it's for good, so it's never mistaken for `F8`.
    #[test]
    fn a_delete_for_good_says_so() {
        let pending = PendingDelete {
            entries: vec![DeleteEntry { path: PathBuf::from("C:/dev/litastum/TODO.md"), name: "TODO.md".into(), is_dir: false, size: 23_300 }],
            permanent: true,
        };
        let theme = Theme::dark();
        for style in [PopupStyle::Rounded, PopupStyle::Classic] {
            let contents = crate::test_support::render_text(80, 24, |frame| draw_confirm_delete_popup(frame, frame.area(), &pending, &theme, style));
            assert!(contents.contains("Delete for good"), "{style:?}");
            assert!(contents.contains("Shift+Del"), "{style:?}");
            assert!(!contents.contains("Recycle Bin"), "{style:?}");
        }
    }

    #[test]
    fn draw_confirm_delete_popup_classic_style_still_shows_the_size() {
        let pending = PendingDelete {
            entries: vec![DeleteEntry { path: PathBuf::from("C:/dev/litastum/TODO.md"), name: "TODO.md".into(), is_dir: false, size: 23_300 }],
            permanent: false,
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
        assert!(contents.contains("To the Recycle Bin"));
        assert!(contents.contains("TODO.md"));
        assert!(contents.contains("23.3 kB"));
    }

}
