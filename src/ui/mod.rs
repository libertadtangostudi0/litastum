use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    Frame,
};

use crate::app::{App, Mode};

mod command_line;
mod compare;
mod compare_line_ending_menu;
mod compare_menu;
mod confirm;
mod drive_menu;
mod editor_find;
mod editor_keymap_menu;
mod editor_menu;
mod editor_pane;
mod find_file;
mod function_keys;
mod image_preview;
mod info;
mod markdown_preview;
mod menu;
mod panel;
mod popup;
mod preview;
mod popup_style_menu;
mod shell;
mod theme_menu;
mod user_menu;

use editor_pane::{draw_confirm_discard_popup, draw_editor};
use function_keys::draw_function_keys;
use info::draw_info_popup;
use panel::draw_panel;

// Only reachable from `ui::tests` (`use super::*;`) -- `draw()` itself
// calls `draw_function_keys`/`draw_panel` directly, never these by
// name, so a non-test build never touches them through this `use`.
#[cfg(test)]
use function_keys::{function_key_columns, ALT_LABELS, DEFAULT_LABELS};
#[cfg(test)]
use panel::build_list_item;

/// Draws the whole application: two file panels side by side, a
/// command-line row, and the F-key hint bar.
///
/// Returns the `(columns, visible_rows)` each panel was actually
/// rendered with, so the caller can feed it back into
/// `Panel::set_columns`/`set_visible_rows` before the next keyboard
/// event is handled — both depend on terminal size, which only
/// `ui::draw` computes, but `Panel` (not `ui`) owns the cursor/scroll
/// state that navigation needs them for.
///
/// Also returns where the real terminal cursor should end up, if
/// anywhere -- `None` means it should stay hidden. **Deliberately not
/// applied via `frame.set_cursor_position` from within this function
/// (or anything it calls) any more** -- every cursor-placing draw
/// function in this app (`editor_pane::draw_editor`, `compare::draw_compare`,
/// `find_file::draw_find_file`, `confirm::draw_confirm_transfer_popup`,
/// ...) now *returns* the position instead, up to `main.rs::run`, which
/// applies it once, itself, after `terminal.draw` has actually finished
/// and the whole frame has reached the terminal.
///
/// This split exists to fix a real, reported flicker: `ratatui`'s own
/// `Terminal::draw` (confirmed directly from its source,
/// `ratatui-core::terminal::render`/`terminal::buffers`) applies
/// `Frame::set_cursor_position` in two *separate* steps after the
/// buffer diff is written -- `show_cursor()`, then `set_cursor_position()`
/// -- and `ratatui-crossterm`'s own backend (also confirmed directly)
/// implements each of those three cursor operations (`hide_cursor`/
/// `show_cursor`/`set_cursor_position`) with `execute!`, which flushes
/// immediately, on its own, rather than queuing alongside the diff the
/// way cell writes themselves do (`queue!`). The result: `show_cursor()`
/// flushes *before* the real target position is applied, briefly making
/// the *real* OS cursor visible at wherever the diff-write's own last
/// `MoveTo` happened to leave it (for an edit that shortens a line --
/// `Delete` at the command line, reported directly -- that's the blank
/// cells written to clear the now-empty tail, i.e. visually the *end*
/// of the line) before the very next flush moves it to the actually
/// intended spot. Applying `set_cursor_position` before `show_cursor`
/// ourselves, once, after the whole frame is already on screen, means
/// the cursor only ever becomes visible already sitting in the right
/// place -- see `main.rs::run`'s own application of this return value.
pub fn draw(frame: &mut Frame, app: &mut App) -> ([(usize, usize); 2], Option<Position>) {
    let theme = app.theme; // Theme is Copy -- see theme.rs for why
    let area = frame.area();
    // Plain `F4` editing (no linked preview) and its own `ConfirmDiscard`
    // still take over the *entire* frame, exactly as before. Once
    // `App::markdown_edit_preview` is `Some` (`F3` on a `.md`/`.markdown`
    // file, `explorer::markdown_preview::open_edit_preview`), both fall
    // through instead to the ordinary panel-layout code below, which
    // draws the editor into the *left* panel's own slot and the live
    // preview into the *right* one -- see `left_columns`/`right_columns`.
    let has_linked_preview = app.markdown_edit_preview.is_some();
    match &mut app.mode {
        Mode::Editing(editor) if !has_linked_preview => {
            let mut cursor = draw_editor(frame, area, editor, &theme);
            if editor.is_searching() {
                // Drawn on top, same "popup over a full-screen mode"
                // shape as ConfirmDiscard below -- and takes over the
                // real terminal cursor from draw_editor's own buffer-
                // cursor placement, same reasoning as the command line's
                // own cursor yielding to whichever popup is showing.
                cursor = Some(editor_find::draw_find_popup(frame, area, editor, &app.search_history, &theme));
            }
            return ([(1, 1), (1, 1)], cursor);
        }
        Mode::ConfirmDiscard(editor) if !has_linked_preview => {
            let cursor = draw_editor(frame, area, editor, &theme);
            draw_confirm_discard_popup(frame, area, &theme);
            return ([(1, 1), (1, 1)], cursor);
        }
        // Same "editor full-screen, popup on top" shape as
        // `ConfirmDiscard` right above -- only ever reached while
        // `!has_linked_preview` (`editor::handle_editor_key` won't open
        // this menu at all otherwise, see `Mode::EditorMenu`'s own doc
        // comment on `app.rs`), so there's no `if has_linked_preview`
        // counterpart to also wire up in the split-view code below, the
        // way `Mode::ConfirmDiscard`/`Mode::Editing` themselves have.
        Mode::EditorMenu(editor, menu) => {
            let cursor = draw_editor(frame, area, editor, &theme);
            editor_menu::draw_editor_menu(frame, area, menu, &theme, app.popup_style);
            return ([(1, 1), (1, 1)], cursor);
        }
        // `EditorMenu`'s own `Keybindings` item -- same shape and same
        // reasoning as `EditorMenu` immediately above.
        Mode::EditorKeymapMenu(editor, menu) => {
            let current = editor.keymap_mode();
            let cursor = draw_editor(frame, area, editor, &theme);
            editor_keymap_menu::draw_editor_keymap_menu(frame, area, menu, &theme, app.popup_style, current);
            return ([(1, 1), (1, 1)], cursor);
        }
        // `Alt+F5`'s own full-screen comparer -- same "takes over the
        // whole frame, `return` before the ordinary 2-panel layout runs
        // at all" shape as `Mode::Editing` above, not the panel-slot
        // shape `Mode::ImagePreview` uses below: a dedicated two-file
        // comparison wants two full-width panes of its own, not one
        // browser panel's worth of space (`TODO/file-compare.md`'s own
        // "Rendering approach"/data-model sketch).
        Mode::CompareFiles(state) => {
            let cursor = compare::draw_compare(frame, area, state, &theme, app.compare_line_ending_display);
            return ([(1, 1), (1, 1)], cursor);
        }
        Mode::CompareMenu(state, menu) => {
            let cursor = compare::draw_compare(frame, area, state, &theme, app.compare_line_ending_display);
            compare_menu::draw_compare_menu(frame, area, menu, &theme, app.popup_style);
            return ([(1, 1), (1, 1)], cursor);
        }
        Mode::CompareLineEndingMenu(state, menu) => {
            let cursor = compare::draw_compare(frame, area, state, &theme, app.compare_line_ending_display);
            compare_line_ending_menu::draw_compare_line_ending_menu(frame, area, menu, &theme, app.popup_style, app.compare_line_ending_display);
            return ([(1, 1), (1, 1)], cursor);
        }
        // Reuses the built-in editor's own "Unsaved changes" popup
        // (`draw_confirm_discard_popup`) unmodified -- it's already
        // generic (just `theme`/`area`, no `Editor` reference), and
        // Compare's own confirm-discard prompt needs exactly the same
        // Y/N choice over whichever screen was showing before `Esc`.
        Mode::CompareConfirmDiscard(state) => {
            let cursor = compare::draw_compare(frame, area, state, &theme, app.compare_line_ending_display);
            draw_confirm_discard_popup(frame, area, &theme);
            return ([(1, 1), (1, 1)], cursor);
        }
        Mode::Browsing
        | Mode::MainMenu(_)
        | Mode::ThemeMenu(_)
        | Mode::ShellMenu(_)
        | Mode::PopupStyleMenu(_)
        | Mode::ConfirmDelete(_)
        | Mode::ConfirmTransfer(_)
        | Mode::FindFile(_)
        | Mode::CommandHistory(_)
        | Mode::ChangeDrive(_)
        | Mode::UserMenu(_)
        | Mode::UserMenuPrompt(_)
        | Mode::ConfirmPortFarMenu(_)
        | Mode::AddUserMenuItem(..)
        | Mode::Info(_)
        | Mode::ImagePreview(_)
        | Mode::Editing(_)
        | Mode::ConfirmDiscard(_)
        | Mode::MarkdownLinkSearch(..) => {}
    }

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    // Keeps the embedded preview scrolled to (and highlighting) roughly
    // the same *relative* line the editor's own cursor is on -- e.g.
    // editing halfway down the editor's own visible page keeps the
    // matching preview line roughly halfway down its own page too,
    // rather than always snapping it to the very top. Requested
    // directly, twice: first to scroll the two panes together and
    // highlight the matching line, then -- once that top-aligned
    // version was actually in use -- to keep them roughly at the same
    // level on the page, so editing in the middle of the page shows the
    // preview at that same middle, not just "together" at the very
    // top. A top-aligned sync technically kept them "together" but put
    // the highlighted line at a different *screen row* than the
    // cursor whenever the cursor wasn't already at the editor's own top
    // line, which is what "on the same level" actually meant. Both
    // heights are derived the same way their own `draw_editor`/
    // `draw_preview_frame` compute their real inner content area
    // (editor: minus the border edtui's own Block draws, minus the
    // one-row hint line below it; preview: minus its own border)
    // rather than duplicating that layout math by guesswork. Gated on
    // `app.active == 0` (the editor
    // has keyboard focus) so Tab-ing over to the preview and scrolling
    // it manually isn't immediately undone the next frame just because
    // the cursor hasn't moved.
    if has_linked_preview && app.active == 0 {
        let cursor_info = match &app.mode {
            Mode::Editing(editor) | Mode::ConfirmDiscard(editor) => Some((editor.cursor_row(), editor.viewport_top_row())),
            _ => None,
        };
        if let Some((cursor_row, viewport_top)) = cursor_info {
            let editor_visible_height = panels[0].height.saturating_sub(3); // border (2) + hint row (1)
            let preview_visible_height = panels[1].height.saturating_sub(2); // border (2)
            let relative_position = if editor_visible_height > 0 {
                (cursor_row.saturating_sub(viewport_top) as f64 / editor_visible_height as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if let Some(preview) = app.markdown_edit_preview.as_mut() {
                preview.sync_to_editor_cursor(cursor_row, relative_position, preview_visible_height as usize);
            }
        }
    }

    // `F3` on a `.md`/`.markdown` file draws the built-in editor into
    // the *left* slot (`App::markdown_edit_preview`'s own doc comment,
    // `Mode::Editing`'s doc comment) instead of that panel's own file
    // listing -- `Mode::MarkdownLinkSearch` parks the editor in its own
    // tuple while its popup is up (below), but the same editor, same
    // slot, still underneath it.
    let left_columns = match &mut app.mode {
        Mode::Editing(editor) | Mode::ConfirmDiscard(editor) if has_linked_preview => {
            draw_editor(frame, panels[0], editor, &theme);
            (1, 1)
        }
        Mode::MarkdownLinkSearch(editor, _) => {
            draw_editor(frame, panels[0], editor, &theme);
            (1, 1)
        }
        _ => draw_panel(frame, panels[0], &app.panels[0], app.active == 0, &theme),
    };
    // `F3` replaces the right panel's own file listing with the
    // previewed image or rendered Markdown entirely
    // (`explorer::image_preview`/`markdown_preview::open_edit_preview`'s
    // own doc comments) -- not a popup drawn over it, unlike every other
    // `Mode` handled in the match below. `Panel::set_columns`/
    // `set_visible_rows` don't matter here: navigation commands never
    // reach the right panel while either preview is showing (each has
    // its own `handle_*_preview_key`/`app.active`-gated dispatch
    // intercepting every key), and the very next frame after closing it
    // recomputes real values again.
    let right_columns = if let Mode::ImagePreview(state) = &mut app.mode {
        // Picks up a decode that finished in the brief window between
        // `wait_for_event`'s own last poll and this draw -- without
        // this, that result would sit ready-and-unused for one extra
        // frame (until the *next* `wait_for_event` poll notices it).
        state.poll();
        image_preview::draw_image_preview(frame, panels[1], state, &theme);
        (1, 1)
    } else if has_linked_preview && matches!(&app.mode, Mode::Editing(_) | Mode::ConfirmDiscard(_) | Mode::MarkdownLinkSearch(..)) {
        // The link-search popup (below) is an overlay over this same
        // underlying preview -- draw it exactly like the plain combined
        // view here, so the document stays visible behind the popup.
        let preview = app.markdown_edit_preview.as_mut().expect("has_linked_preview just confirmed this is Some");
        markdown_preview::draw_markdown_preview(frame, panels[1], preview, &theme);
        (1, 1)
    } else {
        draw_panel(frame, panels[1], &app.panels[1], app.active == 1, &theme)
    };
    let cwd = app.panels[app.active].path.clone();
    let prefix_len = command_line::draw_command_line(frame, root[1], &cwd, &app.command_line, app.command_line_selection_anchor, app.command_line_cursor, &theme);
    draw_function_keys(frame, root[2], &theme, app.alt_held);

    // Auto-popping history suggestions, Far Manager-style: shown right
    // above the command line the instant there's a substring match,
    // no explicit key needed to open it (unlike the Alt+F8 popup,
    // which stays as an always-available manual search). Only in
    // Mode::Browsing -- once a popup/mode below has its own meaning
    // for the command line (or none at all), this shouldn't also be
    // showing over it.
    if matches!(app.mode, Mode::Browsing) && !app.command_line_suggestion_dismissed {
        let suggestions = crate::command_line::suggest_history(&app.command_history, &app.command_line);
        if !suggestions.is_empty() {
            command_line::draw_history_suggestions(frame, root[1], &suggestions, app.command_line_suggestion_selected, &theme);
        }
    }

    // The real terminal cursor sits right after the typed text, same
    // mechanism already used for the editor's cursor (see
    // editor.rs::cursor_screen_position) -- only while nothing else is
    // drawn over the command line (a popup below takes visual priority,
    // and moving the cursor under it would be misleading). `CommandHistory`
    // is the one exception: its popup filters live against this same
    // command line rather than owning a text field of its own (Far
    // Manager's own `Alt+F8` behaves the same way), so the cursor
    // still belongs down here, visible under the popup.
    let mut cursor = if matches!(app.mode, Mode::Browsing | Mode::CommandHistory(_)) {
        Some(Position {
            x: root[1].x + prefix_len + app.command_line_cursor as u16,
            y: root[1].y,
        })
    } else {
        None
    };

    // The F9/Ctrl+P popups show over the browser, like a Far Manager
    // menu, not in place of it -- unlike Editing/ConfirmDiscard above,
    // which replace the whole screen.
    let popup_style = app.popup_style;
    match &app.mode {
        Mode::MainMenu(state) => menu::draw_main_menu(frame, area, state, &theme, popup_style),
        Mode::ThemeMenu(menu) => theme_menu::draw_theme_menu(frame, area, menu, &theme, popup_style),
        Mode::ShellMenu(menu) => shell::draw_shell_menu(frame, area, menu, &app.shell_profiles, &theme, popup_style),
        Mode::PopupStyleMenu(menu) => popup_style_menu::draw_popup_style_menu(frame, area, menu, &theme, popup_style),
        Mode::ConfirmDelete(pending) => confirm::draw_confirm_delete_popup(frame, area, pending, &theme, popup_style),
        Mode::ConfirmTransfer(pending) => {
            cursor = Some(confirm::draw_confirm_transfer_popup(frame, area, pending, &theme, popup_style));
        }
        Mode::FindFile(state) => {
            cursor = find_file::draw_find_file(frame, area, state, &theme, popup_style);
        }
        Mode::CommandHistory(menu) => {
            command_line::draw_command_history(frame, area, menu, &app.command_history, &app.command_line, &theme, popup_style);
        }
        Mode::ChangeDrive(menu) => drive_menu::draw_drive_menu(frame, area, menu, &theme, popup_style),
        Mode::UserMenu(menu) => user_menu::draw_user_menu(frame, area, menu, &theme, popup_style),
        Mode::UserMenuPrompt(prompt) => {
            cursor = Some(user_menu::draw_user_menu_prompt(frame, area, prompt, &theme, popup_style));
        }
        Mode::ConfirmPortFarMenu(far_path) => user_menu::draw_confirm_port_far_menu(frame, area, far_path, &theme, popup_style),
        Mode::AddUserMenuItem(menu, form) => {
            user_menu::draw_user_menu(frame, area, menu, &theme, popup_style);
            cursor = Some(user_menu::draw_add_user_menu_item(frame, area, form, &theme, popup_style));
        }
        Mode::Info(message) => draw_info_popup(frame, area, message, &theme, popup_style),
        Mode::MarkdownLinkSearch(_, search) => {
            cursor = Some(markdown_preview::draw_markdown_link_search(frame, area, search, &theme, popup_style));
        }
        // Both mirror plain `F4`'s own popups (top of this function) --
        // reached here instead because a linked preview
        // (`App::markdown_edit_preview`) sent `Editing`/`ConfirmDiscard`
        // through the ordinary split-panel path above rather than the
        // early, full-screen return.
        Mode::ConfirmDiscard(_) if has_linked_preview => draw_confirm_discard_popup(frame, area, &theme),
        Mode::Editing(editor) if has_linked_preview && editor.is_searching() => {
            cursor = Some(editor_find::draw_find_popup(frame, area, editor, &app.search_history, &theme));
        }
        _ => {}
    }

    ([left_columns, right_columns], cursor)
}


/// A `width`x`height` rectangle centered within `area`, clamped to fit.
/// `pub(crate)` since every popup drawer uses it, and most of those now
/// live in their own mode-owning module (`menu.rs`, `theme_menu.rs`,
/// `shell.rs`, `confirm.rs`) rather than here.
pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}


#[cfg(test)]
mod tests;
