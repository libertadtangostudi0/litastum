use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    Frame,
};

use crate::app::{App, Mode, Overlay};
use crate::theming::Theme;

mod command_line;
mod compare;
mod compare_line_ending_menu;
mod compare_menu;
mod conflict;
mod confirm;
mod drive_menu;
mod editor_find;
mod editor_keymap_menu;
mod editor_menu;
mod editor_pane;
mod find_file;
mod image_preview;
mod info;
mod markdown_preview;
mod menu;
mod notice;
mod panel;
mod path_edit;
mod popup;
mod preview;
mod popup_style_menu;
mod shell;
mod text_field;
mod theme_menu;
mod user_menu;
mod user_screen;

use editor_pane::{draw_confirm_discard_popup, draw_editor};
use info::draw_info_popup;
use panel::draw_panel;
pub use user_screen::{console_rows, draw_console};

// Only reachable from `ui::tests` (`use super::*;`) -- `draw()` itself
// calls `draw_panel` directly, never this by name, so a non-test build
// never touches it through this `use`.
#[cfg(test)]
use panel::build_list_item;

/// Draws the whole app: two panels and the command line under them.
/// Returns the `(columns, visible_rows)` each panel was drawn with --
/// `Panel` owns navigation but only `ui` knows the terminal size -- and
/// where the terminal cursor goes (`None` = hidden). The cursor is
/// returned rather than set with `Frame::set_cursor_position`:
/// `event_loop` places it after the frame is on screen, avoiding a
/// flicker. History: docs/history/event-loop.md.
///
/// A notice toast (`App::notice`) is drawn last, over any screen or popup.
pub fn draw(frame: &mut Frame, app: &mut App) -> ([(usize, usize); 2], Option<Position>) {
    let drawn = draw_screen(frame, app);
    if let Some(notice) = &app.notice {
        notice::draw_notice(frame, notice, &app.theme, app.settings.popup_style);
    }
    drawn
}


fn draw_screen(frame: &mut Frame, app: &mut App) -> ([(usize, usize); 2], Option<Position>) {
    let theme = app.theme; // Theme is Copy -- see theme.rs for why
    let area = frame.area();
    // Plain F4 editing takes the whole frame. With a linked Markdown
    // preview it goes through the panel layout below instead: editor in
    // the left slot, preview in the right.
    let has_linked_preview = app.markdown_edit_preview.is_some();
    // What a full-screen branch returns as the panels' layout: their own
    // current values, so `event_loop`'s apply is a no-op. A placeholder
    // here crammed the panels into one column (docs/history/event-loop.md).
    let unchanged_layout = [(app.panels[0].columns, app.panels[0].visible_rows()), (app.panels[1].columns, app.panels[1].visible_rows())];
    // `Ctrl+O`: the user screen instead of the panels, popups over it.
    if matches!(app.mode, Mode::Browsing) && app.panels_hidden {
        app.user_screen.set_visible_rows(usize::from(console_rows(area.height)));
        let command_line_owns_cursor = matches!(app.overlay, None | Some(Overlay::CommandHistory(_)));
        let mut cursor = draw_console(frame, app, None).filter(|_| command_line_owns_cursor);
        if let Some(overlay_cursor) = draw_overlay(frame, area, app, &theme) {
            cursor = Some(overlay_cursor);
        }
        return (unchanged_layout, cursor);
    }
    match &mut app.mode {
        Mode::Editing(editor) if !has_linked_preview => {
            let mut cursor = draw_editor(frame, area, editor, &theme);
            if let Some(edit) = &app.editor_save_as {
                cursor = path_edit::draw_path_field(frame, editor.title_area(), edit, &theme);
            }
            if editor.search_box_open() {
                // Takes the terminal cursor only while the box has focus.
                let box_cursor = editor_find::draw_find_popup(frame, area, editor, &app.search_history, &theme);
                if editor.is_searching() {
                    cursor = Some(box_cursor);
                }
            }
            draw_overlay(frame, area, app, &theme);
            return (unchanged_layout, cursor);
        }
        // Compare takes the whole frame for its two full-width panes.
        Mode::CompareFiles(state) => {
            let cursor = compare::draw_compare(frame, area, state, &theme, app.settings.compare_line_ending_display);
            draw_overlay(frame, area, app, &theme);
            return (unchanged_layout, cursor);
        }
        // So does the conflict resolver, for its five panes.
        Mode::ResolveConflict(state) => {
            let cursor = conflict::draw_conflict(frame, area, state, &theme, app.settings.compare_line_ending_display);
            draw_overlay(frame, area, app, &theme);
            return (unchanged_layout, cursor);
        }
        Mode::Browsing | Mode::ImagePreview(_) | Mode::Editing(_) => {}
    }

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    // Keeps the linked preview's highlighted line level with the editor's
    // cursor on the page. Heights mirror `draw_editor`'s (border + hint
    // row) and `draw_preview_frame`'s (border) content areas. Only while
    // the editor has focus, so a manual preview scroll isn't undone.
    // History: docs/history/markdown-preview.md.
    if has_linked_preview && app.active == 0 {
        let cursor_info = match &app.mode {
            Mode::Editing(editor) => Some((editor.cursor_row(), editor.viewport_top_row())),
            _ => None,
        };
        if let Some((cursor_row, viewport_top)) = cursor_info {
            let editor_visible_height = panels[0].height.saturating_sub(2); // border
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
    // listing -- also under the link-search overlay.
    let mut save_as_cursor = None;
    // Where each panel lands, for a click on its title; nowhere while the
    // editor or a preview takes its place.
    let editor_split = has_linked_preview && matches!(app.mode, Mode::Editing(_));
    let panel_drawn = [!editor_split, !editor_split && !matches!(app.mode, Mode::ImagePreview(_))];
    for (panel, (drawn, area)) in app.panels.iter_mut().zip(panel_drawn.into_iter().zip(panels.iter())) {
        panel.screen_area = if drawn { *area } else { Rect::default() };
    }
    let left_columns = match &mut app.mode {
        Mode::Editing(editor) if has_linked_preview => {
            draw_editor(frame, panels[0], editor, &theme);
            if let Some(edit) = &app.editor_save_as {
                save_as_cursor = path_edit::draw_path_field(frame, editor.title_area(), edit, &theme);
            }
            (1, 1)
        }
        _ => draw_panel(frame, panels[0], &app.panels[0], app.active == 0, &theme),
    };
    // `F3` replaces the right panel's listing with the image or the
    // Markdown preview. Its reported layout doesn't matter meanwhile: the
    // preview takes the keys, and the next frame after closing is real.
    let right_columns = if let Mode::ImagePreview(state) = &mut app.mode {
        // Picks up a decode that finished since the last poll.
        state.poll();
        image_preview::draw_image_preview(frame, panels[1], state, &theme);
        (1, 1)
    } else if has_linked_preview && matches!(&app.mode, Mode::Editing(_)) {
        let preview = app.markdown_edit_preview.as_mut().expect("has_linked_preview just confirmed this is Some");
        markdown_preview::draw_markdown_preview(frame, panels[1], preview, &theme);
        (1, 1)
    } else {
        draw_panel(frame, panels[1], &app.panels[1], app.active == 1, &theme)
    };
    let command_cursor = draw_command_rows(frame, root[1], app, &theme, true);

    // The cursor sits in the command line unless a popup covers it --
    // except the history popup, which filters that same line.
    let command_line_owns_cursor = matches!(app.overlay, None | Some(Overlay::CommandHistory(_)));
    let mut cursor = (matches!(app.mode, Mode::Browsing) && command_line_owns_cursor).then_some(command_cursor);
    // The panel's path field (`Ctrl+L`) over its top border takes the
    // cursor from the command line while it's open.
    if let (Some(edit), Mode::Browsing, true) = (&app.panel_path_edit, &app.mode, command_line_owns_cursor) {
        let panel = panels[app.active];
        let title = Rect::new(panel.x + 1, panel.y, panel.width.saturating_sub(2), 1);
        cursor = path_edit::draw_path_field(frame, title, edit, &theme);
    }

    // Mirrors plain `F4`'s own search box (top of this function) --
    // reached here because a linked preview sent `Editing` through the
    // split-panel path instead of the full-screen return.
    if let Mode::Editing(editor) = &app.mode {
        if has_linked_preview && editor.search_box_open() {
            let box_cursor = editor_find::draw_find_popup(frame, area, editor, &app.search_history, &theme);
            if editor.is_searching() {
                cursor = Some(box_cursor);
            }
        }
    }
    if save_as_cursor.is_some() {
        cursor = save_as_cursor;
    }
    if let Some(overlay_cursor) = draw_overlay(frame, area, app, &theme) {
        cursor = Some(overlay_cursor);
    }

    ([left_columns, right_columns], cursor)
}


/// The command line -- under the panels and under the user screen alike
/// -- with the suggestions (history, panel names) popping up over it on a
/// match, as in Far, on the bare browser only and while `suggest` (not
/// while a command runs: its keys are the program's). No F-key bar under
/// it (dropped as noise, requested). Returns where its cursor is.
fn draw_command_rows(frame: &mut Frame, line_row: Rect, app: &App, theme: &Theme, suggest: bool) -> Position {
    let cwd = app.panels[app.active].path.clone();
    let prefix_len = command_line::draw_command_line(frame, line_row, &cwd, &app.command_line, theme);
    if suggest && matches!(app.mode, Mode::Browsing) && app.overlay.is_none() && app.panel_path_edit.is_none() && !app.command_line_suggestion_dismissed {
        let suggestions = crate::command_line::suggestions(app);
        if !suggestions.is_empty() {
            command_line::draw_suggestions(frame, line_row, &suggestions, app.command_line_suggestion_selected, theme);
        }
    }
    Position { x: line_row.x + prefix_len + app.command_line.cursor() as u16, y: line_row.y }
}


/// Draws `app.overlay`, if any, over whatever screen was just drawn --
/// popups show over the browser, editor or Compare like a Far menu, not
/// in place of them. Returns where the terminal cursor goes when the
/// overlay has a text field of its own.
fn draw_overlay(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) -> Option<Position> {
    let overlay = app.overlay.as_ref()?;
    let style = app.settings.popup_style;
    match overlay {
        Overlay::ConfirmDiscard => draw_confirm_discard_popup(frame, area, theme, style),
        Overlay::EditorMenu(menu) => editor_menu::draw_editor_menu(frame, area, menu, theme, style),
        Overlay::EditorKeymapMenu(menu) => {
            let current = match &app.mode {
                Mode::Editing(editor) => editor.keymap_mode(),
                _ => app.settings.editor_keymap_mode,
            };
            editor_keymap_menu::draw_editor_keymap_menu(frame, area, menu, theme, style, current);
        }
        Overlay::CompareMenu(menu) => compare_menu::draw_compare_menu(frame, area, menu, theme, style),
        Overlay::CompareLineEndingMenu(menu) => {
            compare_line_ending_menu::draw_compare_line_ending_menu(frame, area, menu, theme, style, app.settings.compare_line_ending_display);
        }
        Overlay::MainMenu(state) => menu::draw_main_menu(frame, area, state, theme, style),
        Overlay::ThemeMenu(menu) => theme_menu::draw_theme_menu(frame, area, menu, theme, style),
        Overlay::ShellMenu(menu) => shell::draw_shell_menu(frame, area, menu, &app.shell_profiles, theme, style),
        Overlay::PopupStyleMenu(menu) => popup_style_menu::draw_popup_style_menu(frame, area, menu, theme, style),
        Overlay::ConfirmDelete(pending) => confirm::draw_confirm_delete_popup(frame, area, pending, theme, style),
        Overlay::ConfirmTransfer(pending) => return Some(confirm::draw_confirm_transfer_popup(frame, area, pending, theme, style)),
        Overlay::FindFile(state) => return find_file::draw_find_file(frame, area, state, theme, style),
        // Filters against the command line underneath, whose cursor stays.
        Overlay::CommandHistory(menu) => {
            command_line::draw_command_history(frame, area, menu, &app.command_history, app.command_line.text(), theme, style);
        }
        Overlay::ChangeDrive(menu) => drive_menu::draw_drive_menu(frame, area, menu, theme, style),
        Overlay::UserMenu(menu) => user_menu::draw_user_menu(frame, area, menu, theme, style),
        Overlay::UserMenuPrompt(prompt) => return Some(user_menu::draw_user_menu_prompt(frame, area, prompt, theme, style)),
        Overlay::ConfirmPortFarMenu(far_path) => user_menu::draw_confirm_port_far_menu(frame, area, far_path, theme, style),
        Overlay::AddUserMenuItem(menu, form) => {
            user_menu::draw_user_menu(frame, area, menu, theme, style);
            return Some(user_menu::draw_add_user_menu_item(frame, area, form, theme, style));
        }
        Overlay::Info(message) => draw_info_popup(frame, area, message, theme, style),
        Overlay::MarkdownLinkSearch(search) => return Some(markdown_preview::draw_markdown_link_search(frame, area, search, theme, style)),
    }
    None
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
