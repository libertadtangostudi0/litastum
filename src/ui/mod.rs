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
mod text_field;
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
/// ...) now *returns* the position instead, up to `event_loop::run`, which
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
/// place -- see `event_loop::run`'s own application of this return value.
pub fn draw(frame: &mut Frame, app: &mut App) -> ([(usize, usize); 2], Option<Position>) {
    let theme = app.theme; // Theme is Copy -- see theme.rs for why
    let area = frame.area();
    // Plain `F4` editing (no linked preview) takes over the *entire*
    // frame. Once `App::markdown_edit_preview` is `Some` (`F3` on a
    // `.md`/`.markdown` file, `explorer::markdown_preview::open_edit_preview`),
    // it falls through instead to the ordinary panel-layout code below, which
    // draws the editor into the *left* panel's own slot and the live
    // preview into the *right* one -- see `left_columns`/`right_columns`.
    let has_linked_preview = app.markdown_edit_preview.is_some();
    // What every early-return branch below hands back in place of a
    // real, freshly-rendered `(columns, visible_rows)` pair -- one of
    // these modes (the built-in editor, Compare) doesn't draw either
    // panel at all, so there's nothing new to report. Reported directly
    // as a real, persistent glitch, not just a one-frame flash: closing
    // the editor showed a single entry crammed into one narrow column
    // for one whole extra frame, the same shape `event_loop::run`'s own
    // priming-draw comment already describes for startup. Root cause
    // here was the same "one frame stale" timing, just recurring on
    // every full-screen-mode exit instead of only at startup: this
    // function used to return the *placeholder* `[(1, 1), (1, 1)]`
    // itself, which `event_loop::run`'s loop then applied to *both* panels
    // via `Panel::set_columns`/`set_visible_rows` on *every single
    // frame* the editor/Compare stayed open -- clobbering their real
    // values down to a forced single column/row the whole time, not
    // just leaving them stale. Reporting each panel's own
    // already-known values instead (nothing panel-specific actually
    // changed just because this frame drew something else) makes that
    // blind apply a harmless no-op until a real panel frame is drawn
    // again.
    let unchanged_layout = [(app.panels[0].columns, app.panels[0].visible_rows()), (app.panels[1].columns, app.panels[1].visible_rows())];
    match &mut app.mode {
        Mode::Editing(editor) if !has_linked_preview => {
            let mut cursor = draw_editor(frame, area, editor, &theme);
            if editor.search_box_open() {
                // Drawn on top, like an overlay -- and, only while the
                // box has keyboard focus, takes over the real terminal
                // cursor from draw_editor's own buffer-cursor placement
                // (with focus in the text, the caret there is the one
                // that should show).
                let box_cursor = editor_find::draw_find_popup(frame, area, editor, &app.search_history, &theme);
                if editor.is_searching() {
                    cursor = Some(box_cursor);
                }
            }
            draw_overlay(frame, area, app, &theme);
            return (unchanged_layout, cursor);
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
            Mode::Editing(editor) => Some((editor.cursor_row(), editor.viewport_top_row())),
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
    // listing -- also under the link-search overlay.
    let left_columns = match &mut app.mode {
        Mode::Editing(editor) if has_linked_preview => {
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
    } else if has_linked_preview && matches!(&app.mode, Mode::Editing(_)) {
        let preview = app.markdown_edit_preview.as_mut().expect("has_linked_preview just confirmed this is Some");
        markdown_preview::draw_markdown_preview(frame, panels[1], preview, &theme);
        (1, 1)
    } else {
        draw_panel(frame, panels[1], &app.panels[1], app.active == 1, &theme)
    };
    let cwd = app.panels[app.active].path.clone();
    let prefix_len = command_line::draw_command_line(frame, root[1], &cwd, &app.command_line, &theme);
    draw_function_keys(frame, root[2], &theme, app.alt_held);

    // Auto-popping history suggestions, Far Manager-style: shown right
    // above the command line the instant there's a substring match,
    // no explicit key needed to open it (unlike the Alt+F8 popup,
    // which stays as an always-available manual search). Only on the
    // bare browser -- under a popup it would show through.
    if matches!(app.mode, Mode::Browsing) && app.overlay.is_none() && !app.command_line_suggestion_dismissed {
        let suggestions = crate::command_line::suggest_history(&app.command_history, app.command_line.text());
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
    let command_line_owns_cursor = matches!(app.overlay, None | Some(Overlay::CommandHistory(_)));
    let mut cursor = if matches!(app.mode, Mode::Browsing) && command_line_owns_cursor {
        Some(Position {
            x: root[1].x + prefix_len + app.command_line.cursor() as u16,
            y: root[1].y,
        })
    } else {
        None
    };

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
    if let Some(overlay_cursor) = draw_overlay(frame, area, app, &theme) {
        cursor = Some(overlay_cursor);
    }

    ([left_columns, right_columns], cursor)
}


/// Draws `app.overlay`, if any, over whatever screen was just drawn --
/// popups show over the browser, editor or Compare like a Far menu, not
/// in place of them. Returns where the terminal cursor goes when the
/// overlay has a text field of its own.
fn draw_overlay(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) -> Option<Position> {
    let overlay = app.overlay.as_ref()?;
    let style = app.popup_style;
    match overlay {
        Overlay::ConfirmDiscard => draw_confirm_discard_popup(frame, area, theme),
        Overlay::EditorMenu(menu) => editor_menu::draw_editor_menu(frame, area, menu, theme, style),
        Overlay::EditorKeymapMenu(menu) => {
            let current = match &app.mode {
                Mode::Editing(editor) => editor.keymap_mode(),
                _ => app.editor_keymap_mode,
            };
            editor_keymap_menu::draw_editor_keymap_menu(frame, area, menu, theme, style, current);
        }
        Overlay::CompareMenu(menu) => compare_menu::draw_compare_menu(frame, area, menu, theme, style),
        Overlay::CompareLineEndingMenu(menu) => {
            compare_line_ending_menu::draw_compare_line_ending_menu(frame, area, menu, theme, style, app.compare_line_ending_display);
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
