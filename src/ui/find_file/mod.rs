use std::sync::atomic::Ordering;

use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::explorer::{FindFileField, FindFilePhase, FindFileState};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders whichever phase is current. Returns where the real terminal
/// cursor should sit — only meaningful during `Typing` (same mechanism
/// as the command line's own cursor, `ui::draw`); `None` during
/// `Searching`/`Results`, neither of which has a text entry to place a
/// cursor in.
pub fn draw_find_file(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) -> Option<Position> {
    match state.phase {
        FindFilePhase::Typing => Some(draw_typing(frame, area, state, theme, style)),
        FindFilePhase::Searching => {
            draw_searching(frame, area, state, theme, style);
            None
        }
        FindFilePhase::Results => {
            draw_results(frame, area, state, theme, style);
            None
        }
    }
}

/// "1 result" / "N results" / "N+ results" -- the plural-agreement half
/// of the results title's own count-and-timing summary. `capped`
/// (`FindFileState::results_capped`) appends a "+", the same widely
/// understood "there's more, this isn't the real total" convention a
/// lot of search/notification UIs already use -- added directly after
/// a report that a search hitting exactly `find_file_max_results` (200,
/// the default) looked like a complete, successful search with no way
/// to tell it wasn't, compared side by side against real Far Manager
/// finding well over double that on the same tree.
fn result_count_label(count: usize, capped: bool) -> String {
    let plus = if capped { "+" } else { "" };
    if count == 1 && !capped {
        "1 result".to_string()
    } else {
        format!("{count}{plus} results")
    }
}

/// Sub-second durations render as whole milliseconds (a decimal second
/// value would show mostly zeroes for the common, fast case); a full
/// second or more switches to seconds with two decimal places, which
/// is roughly the precision a human glancing at a search's own timing
/// actually cares about either way.
fn format_duration(duration: std::time::Duration) -> String {
    if duration.as_secs() >= 1 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn draw_typing(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) -> Position {
    // `Rounded` gets a separator right under the title, matching
    // `ui/theme_menu.rs`'s own color-scheme picker (the reference look
    // requested directly) -- `Classic` stays as plain as every other
    // Classic-style popup, no separator of its own.
    let has_separator = style == PopupStyle::Rounded;
    // 5 content rows (name label, name value, content label, content
    // value, hint) -- one label+value pair per field, matching Far
    // Manager's own two-field Find file dialog (name mask, and a
    // separate "Text to find" for searching inside files) -- plus the
    // separator's own row when there is one, plus 2 for a plain border.
    // `Rounded`'s own extra padding/title chrome needs more room for the
    // same rows, or they'd get clipped to nothing
    // (`popup::chrome_extra_rows`'s own doc comment has the exact
    // accounting).
    let height = 7 + popup::chrome_extra_rows(style) + u16::from(has_separator);
    // `Classic` bakes " Find file " (with its own margin spaces) into
    // the border; `Rounded` already gets real margin from `draw_frame`'s
    // own padding, so an un-padded bold title -- matching `ui/theme_menu
    // .rs`'s "Color scheme" exactly -- avoids stacking that margin twice
    // and sitting one column further right than every other Rounded
    // popup's title.
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Find file ")),
        PopupStyle::Rounded => Line::from(Span::styled("Find file", Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, popup::percent_width(area, 80), height);

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    if has_separator {
        constraints.insert(0, Constraint::Length(1));
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separator);
    if has_separator {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
    }

    // The active field's own label is bold/accented so it's clear which
    // field `Tab` and typed characters currently reach -- the inactive
    // one stays plain, same visual weight every other unfocused label in
    // this app already gets.
    let label_style = |field: FindFileField| {
        if state.active_field == field {
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        }
    };

    let name_label = Line::from(Span::styled("File name to find:", label_style(FindFileField::Name)));
    frame.render_widget(name_label, rows[content_start]);

    let name_row = rows[content_start + 1];
    frame.render_widget(super::text_field::field_line(&state.query, theme), name_row);

    let content_label = Line::from(Span::styled("Text to find:", label_style(FindFileField::Content)));
    frame.render_widget(content_label, rows[content_start + 2]);

    let content_row = rows[content_start + 3];
    frame.render_widget(super::text_field::field_line(&state.content_query, theme), content_row);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" search  ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" switch field  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[content_start + 4]);

    let (cursor_row, cursor) = match state.active_field {
        FindFileField::Name => (name_row, state.query.cursor()),
        FindFileField::Content => (content_row, state.content_query.cursor()),
    };
    Position { x: cursor_row.x + cursor as u16, y: cursor_row.y }
}

/// `FindFilePhase::Searching`: a live "please wait" screen shown while
/// the background search (`background.rs`) is still running --
/// requested directly for a comparison against real Far Manager's own
/// dialog, which shows the same kind of thing rather than blocking with
/// nothing on screen. Reads `state.pending`'s own live counters
/// directly (`SearchProgress`, `Relaxed` loads -- this is just a status
/// line refreshed every redraw, not something anything synchronizes
/// real work on) rather than needing its own copy of the numbers.
fn draw_searching(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) {
    let has_separator = style == PopupStyle::Rounded;
    // 2 content rows (status line, hint) -- deliberately terser than
    // `Typing`'s 5, since there's nothing to edit here.
    let height = 4 + popup::chrome_extra_rows(style) + u16::from(has_separator);
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Find file ")),
        PopupStyle::Rounded => Line::from(Span::styled("Find file", Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, popup::percent_width(area, 80), height);

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1)];
    if has_separator {
        constraints.insert(0, Constraint::Length(1));
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separator);
    if has_separator {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
    }

    // `state.pending` should always be `Some` here -- `Searching` is
    // only ever entered alongside setting it (`input.rs::run_search`)
    // -- but this degrades to a plain "Searching..." with zero counts
    // rather than panicking if that invariant were ever somehow broken.
    let status_text = match &state.pending {
        Some(pending) => {
            let visited = pending.progress.visited.load(Ordering::Relaxed);
            let found = pending.progress.found.load(Ordering::Relaxed);
            format!("Searching... {visited} visited, {found} found ({})", format_duration(pending.started.elapsed()))
        }
        None => "Searching...".to_string(),
    };
    let status = Line::from(Span::styled(status_text, Style::default().fg(theme.text)));
    frame.render_widget(status, rows[content_start]);

    let hint = Line::from(vec![
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[content_start + 1]);
}

/// Fixed popup height, independent of `state.results.len()` -- now that
/// the list actually scrolls (see the `ListState` below), there's no
/// reason for the popup itself to keep growing with the result count
/// the way it used to. Picked to match `ui/theme_menu.rs`'s own
/// color-scheme popup height directly, per an explicit side-by-side
/// comparison request -- that popup's own formula (`themes.len() + 13`)
/// comes out to `21` for the 8 themes bundled with this repo, so this
/// reuses that same number rather than inventing an unrelated one.
const RESULTS_HEIGHT: u16 = 21;

fn draw_results(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme, style: PopupStyle) {
    // `Rounded` gets a bold title with a separator right under it
    // (matching `ui/theme_menu.rs`'s own color-scheme picker, the
    // reference requested directly) and another separator right before
    // the footer hints (matching `ui/confirm.rs`'s delete popup) --
    // `Classic` stays plain, title baked into the border, no separators
    // of its own, same as every other Classic-style popup.
    let has_separators = style == PopupStyle::Rounded;
    let height = (RESULTS_HEIGHT + 2 * u16::from(has_separators)).min(area.height);
    // A content query, when set, is folded into the title too -- Far
    // Manager's own results view names both the mask and the searched
    // text once a search runs.
    let base_title = if state.content_query.is_empty() {
        format!("Find file: \"{}\"", state.query.text())
    } else {
        format!("Find file: \"{}\" containing \"{}\"", state.query.text(), state.content_query.text())
    };
    // The result count and how long the search actually took -- Far
    // Manager's own dialog shows both once a search finishes, requested
    // directly here too. `search_duration` is `None` only if this popup
    // somehow reached `Results` without ever calling `run_search`
    // (doesn't happen in practice, but the title degrades gracefully
    // rather than showing a fabricated "0ms" if it ever did).
    let title_text = match state.search_duration {
        Some(duration) => format!("{base_title} — {} in {}", result_count_label(state.results.len(), state.results_capped), format_duration(duration)),
        None => base_title,
    };
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(format!(" {title_text} "))),
        PopupStyle::Rounded => Line::from(Span::styled(title_text, Style::default().fg(theme.text).add_modifier(Modifier::BOLD))),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, popup::percent_width(area, 80), height);

    // Two fixed rows for the export message (label + detail, e.g.
    // "Exported to:" / the actual path) even when there isn't one --
    // simpler than resizing the popup depending on whether a message
    // is currently showing, at the cost of a little blank space in the
    // common "haven't exported yet" case.
    let mut constraints = vec![Constraint::Min(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    if has_separators {
        constraints.insert(0, Constraint::Length(1)); // separator under the title
        constraints.insert(4, Constraint::Length(1)); // separator before the hints
    }
    let rows = Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let content_start = usize::from(has_separators);
    let hint_row_index = if has_separators {
        frame.render_widget(popup::separator(inner.width, theme), rows[0]);
        frame.render_widget(popup::separator(inner.width, theme), rows[4]);
        5
    } else {
        3
    };

    if state.results.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled("No matches found", Style::default().fg(theme.text_dim))));
        frame.render_widget(empty, rows[content_start]);
    } else {
        let items: Vec<ListItem> = state
            .results
            .iter()
            .enumerate()
            .map(|(index, path)| {
                // `theme.warning` for a marked (but not currently
                // selected) row -- same "attention/marked" color and
                // bold weight `Panel::marked_entries`' own rendering
                // (`ui/panel.rs::build_list_item`) already uses, so
                // marking a result reads the same way marking a panel
                // entry does. The selected row keeps its own highlight
                // regardless of whether it's also marked, matching that
                // same panel code's own "selection wins" precedent.
                let style = if index == state.selected {
                    popup::selected_row_style(theme)
                } else if state.marked.contains(&index) {
                    Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(path.to_string_lossy().into_owned(), style)))
            })
            .collect();
        // A result count in the thousands (a big tree, a broad query)
        // used to render every item straight into this fixed-height
        // area with no scroll offset at all -- reported directly, the
        // same "List with no ListState doesn't auto-scroll" gap
        // `Panel`'s own entry grid hit earlier (see its own doc comment
        // in `explorer/panel/mod.rs`), and already fixed once in this
        // codebase for `ui/theme_menu.rs`'s picker the same way: a real
        // `ListState` tracking the selected index, which `List` then
        // scrolls to keep in view on its own.
        let list = List::new(items);
        let mut list_state = ListState::default().with_selected(Some(state.selected));
        frame.render_stateful_widget(list, rows[content_start], &mut list_state);
    }

    if let Some((label, detail)) = &state.export_message {
        let label_line = Line::from(Span::styled(label.clone(), Style::default().fg(theme.text_dim)));
        frame.render_widget(label_line, rows[content_start + 1]);
        // A real Downloads path is easily wide enough to overflow the
        // popup on one line together with the label -- ratatui clips
        // rather than wraps a `Line` that's too long for its area, so
        // splitting the detail onto its own row (still just clipped if
        // it's *itself* wider than the popup, but that's a much rarer
        // case than "label + path together" was) is the fix here, not
        // a text-wrapping widget for what's meant to be a one-line
        // status.
        let detail_line = Line::from(Span::styled(detail.clone(), Style::default().fg(theme.text)));
        frame.render_widget(detail_line, rows[content_start + 2]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" go to  ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" peek  ", Style::default().fg(theme.text_dim)),
        Span::styled("F4", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" edit  ", Style::default().fg(theme.text_dim)),
        Span::styled("Shift+↑↓", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" mark  ", Style::default().fg(theme.text_dim)),
        Span::styled("Alt+F5", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" compare  ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+S", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" export  ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+C", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" copy path  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[hint_row_index]);
}


#[cfg(test)]
mod tests;
