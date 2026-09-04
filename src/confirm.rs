//! Key handling *and rendering* for the two ephemeral filesystem-action
//! popups: F8's "delete this?" (`Mode::ConfirmDelete`) and
//! F5/F6/Shift+F6's "copy/move/rename to?" (`Mode::ConfirmTransfer`).
//! Grouped in one module since both are small, short-lived
//! confirmations over a `Pending*` struct from `app.rs`, and neither
//! owns a whole subsystem the way `theme_menu.rs`/`menu.rs`/
//! `command_line.rs` do — moved out of `main.rs`/`ui.rs` for the same
//! reason those were: each mode's key handling and rendering lives
//! with the concern it belongs to, rather than in a general dispatcher.

use std::fs;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use tracing::debug;

use crate::app::{App, Mode, PendingDelete, PendingTransfer, TransferOp};
use crate::theme::Theme;
use crate::ui::centered_rect;
use crate::{fs_ops, keymap, text_field};


/// Key handling on the F8 "delete this?" prompt: `Y` actually deletes
/// (a file via `fs::remove_file`, a directory recursively via
/// `fs::remove_dir_all` — no separate "is it empty" case, matching Far
/// Manager's own F8 which recurses without asking twice) and reloads
/// the panel; `N`/`Esc` cancels with nothing touched. A failed delete
/// (permissions, a file in use, ...) is logged rather than crashing —
/// there's no status-bar message surface yet to show it to the user
/// (see `TODO.md`'s non-UTF-8-file gap, same underlying limitation).
pub fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use keymap::ConfirmDeleteCommand;

    let Mode::ConfirmDelete(pending) = &app.mode else {
        return Ok(());
    };

    let command = keymap::resolve_confirm_delete(key);
    debug!(?key, ?command, path = %pending.path.display(), "confirm-delete key");

    match command {
        ConfirmDeleteCommand::Confirm => {
            let Mode::ConfirmDelete(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::ConfirmDelete above");
            };
            let result = if pending.is_dir {
                fs::remove_dir_all(&pending.path)
            } else {
                fs::remove_file(&pending.path)
            };
            if let Err(err) = result {
                debug!(path = %pending.path.display(), %err, "delete failed");
            }
            app.active_panel().reload()?;
        }
        ConfirmDeleteCommand::Cancel => app.mode = Mode::Browsing,
        ConfirmDeleteCommand::Ignore => {}
    }

    Ok(())
}


/// Key handling on the F5/F6 "copy/move to?" prompt: the destination
/// line gets a real cursor (`text_field.rs`, not the command line's
/// own append/backspace-only editing — see that module's doc for why
/// this popup gets one and the always-live command line doesn't) —
/// `Left`/`Right` move a character, `Ctrl+Left`/`Ctrl+Right` a word,
/// `Home`/`End` to the edges, `Backspace`/`Delete` remove around the
/// cursor. `Enter` performs the transfer (`fs_ops::copy_entry`/
/// `move_entry`) and reloads *both* panels (the destination side
/// always needs it, and a move also changes the source side); `Esc`
/// cancels with nothing touched. A failed transfer is only logged, same
/// as `handle_confirm_delete_key` — no status-bar surface exists yet.
pub fn handle_confirm_transfer_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Enter {
        return run_confirmed_transfer(app);
    }
    if key.code == KeyCode::Esc {
        app.mode = Mode::Browsing;
        return Ok(());
    }

    // Everything past this point only edits the destination field, so
    // borrow `pending` once instead of re-matching `Mode::ConfirmTransfer`
    // per key (each of the arms below used to do its own `if let`).
    let Mode::ConfirmTransfer(pending) = &mut app.mode else {
        return Ok(());
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        // Backspace/Delete remove the active selection instead of one
        // character, if there is one -- text_field::delete_selection
        // reports whether it did anything, so the single-character path
        // only runs when there wasn't a selection to consume instead.
        KeyCode::Backspace => {
            let removed_selection =
                text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            if !removed_selection {
                text_field::backspace(&mut pending.destination, &mut pending.cursor);
            }
        }
        KeyCode::Delete => {
            let removed_selection =
                text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            if !removed_selection {
                text_field::delete_forward(&mut pending.destination, &mut pending.cursor);
            }
        }
        // Shift+Left/Right (selection) is checked ahead of Ctrl+Left/
        // Right and plain Left/Right below, same reason Ctrl+P is
        // checked ahead of the browsing keymap table -- KeyCode::Left
        // alone can't distinguish "extend selection" from "move" or
        // "jump a word".
        KeyCode::Left if shift => {
            text_field::extend_selection_left(&mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Right if shift => {
            text_field::extend_selection_right(&pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Left if ctrl => {
            pending.selection_anchor = None;
            text_field::move_word_left(&pending.destination, &mut pending.cursor);
        }
        KeyCode::Right if ctrl => {
            pending.selection_anchor = None;
            text_field::move_word_right(&pending.destination, &mut pending.cursor);
        }
        // Plain Left/Right with a selection active collapses to that
        // selection's near edge (standard editor behavior) rather than
        // moving one further character past it.
        KeyCode::Left => {
            text_field::collapse_selection_left(&mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Right => {
            text_field::collapse_selection_right(&pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
        }
        KeyCode::Home => {
            pending.selection_anchor = None;
            text_field::move_home(&mut pending.cursor);
        }
        KeyCode::End => {
            pending.selection_anchor = None;
            text_field::move_end(&pending.destination, &mut pending.cursor);
        }
        // Typing over an active selection replaces it, like any normal
        // text field -- delete it first, then insert at the (now
        // collapsed) cursor.
        KeyCode::Char(c) if !ctrl => {
            text_field::delete_selection(&mut pending.destination, &mut pending.cursor, &mut pending.selection_anchor);
            text_field::insert_char(&mut pending.destination, &mut pending.cursor, c);
        }
        _ => {}
    }

    Ok(())
}


/// `Enter` on the transfer prompt: runs the copy/move
/// (`fs_ops::copy_entry`/`move_entry`) and reloads both panels. Split
/// out of `handle_confirm_transfer_key` since it needs to consume
/// `app.mode` via `mem::replace` (to take ownership of `PendingTransfer`
/// without cloning it) rather than just borrow it like every other key
/// on that prompt does.
fn run_confirmed_transfer(app: &mut App) -> Result<()> {
    let Mode::ConfirmTransfer(pending) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        return Ok(());
    };
    let destination = PathBuf::from(pending.destination.trim());
    debug!(
        source = %pending.source.display(),
        destination = %destination.display(),
        op = ?pending.operation,
        "confirm-transfer: running"
    );
    let result = match pending.operation {
        TransferOp::Copy => fs_ops::copy_entry(&pending.source, &destination, pending.is_dir),
        TransferOp::Move => fs_ops::move_entry(&pending.source, &destination, pending.is_dir),
    };
    if let Err(err) = result {
        debug!(source = %pending.source.display(), destination = %destination.display(), %err, "transfer failed");
    }
    for panel in &mut app.panels {
        panel.reload()?;
    }
    Ok(())
}


/// Renders the F8 "delete this?" prompt over the browser. Moved here
/// from `ui.rs` alongside the key handling above.
pub fn draw_confirm_delete_popup(frame: &mut Frame, area: Rect, pending: &PendingDelete, theme: &Theme) {
    let popup = centered_rect(50, 4, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.danger))
        .title(" Delete ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let question = if pending.is_dir {
        format!("Delete directory '{}' and all its contents?", pending.name)
    } else {
        format!("Delete '{}'?", pending.name)
    };

    let lines = vec![
        Line::from(Span::styled(question, Style::default().fg(theme.text))),
        Line::from(vec![
            Span::styled("Y", Style::default().fg(theme.danger).add_modifier(Modifier::BOLD)),
            Span::styled(" delete    ", Style::default().fg(theme.text_dim)),
            Span::styled("N", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" / Esc cancel", Style::default().fg(theme.text_dim)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}


/// Renders the F5/F6 "copy/move to?" prompt: source name and operation
/// on one line, an editable destination path on the next (defaults to
/// the other panel's directory — see `command::request_transfer`).
/// Returns where the real terminal cursor should sit, same mechanism as
/// the command line's own cursor (`ui::draw`). Moved here from `ui.rs`
/// alongside the key handling above.
pub fn draw_confirm_transfer_popup(frame: &mut Frame, area: Rect, pending: &PendingTransfer, theme: &Theme) -> Position {
    let popup = centered_rect(60, 6, area);

    frame.render_widget(Clear, popup);

    let verb = match pending.operation {
        TransferOp::Copy => "Copy",
        TransferOp::Move => "Move",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(format!(" {verb} "));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let source_line = Line::from(Span::styled(
        format!("{verb} '{}' to:", pending.name),
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
