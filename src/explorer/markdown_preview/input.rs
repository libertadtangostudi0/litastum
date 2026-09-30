use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::editor::{self, Editor};

use super::links::{open_link, MarkdownLinkSearchState};
use super::state::MarkdownPreviewState;

/// `F3` on a `.md` file: the editor (as `F4`) and a live preview side by
/// side (`App::markdown_edit_preview`). Focus starts in the editor; `Tab`
/// switches to the preview. A no-op if either fails to open. Mouse
/// capture comes with the editor (`event_loop::sync_mouse_capture`).
pub fn open_edit_preview(app: &mut App) {
    let Some(path) = app.active_panel().selected_path() else {
        return;
    };

    let Some(preview) = MarkdownPreviewState::open(&path) else {
        return;
    };
    let syntax_theme = app.syntax_theme.clone();
    let Ok(editor) = Editor::open(path, syntax_theme, app.settings.editor_keymap_mode) else {
        return;
    };

    app.markdown_edit_preview = Some(preview);
    app.active = 0;
    app.mode = Mode::Editing(editor);
}


/// Keys while the preview has focus (`app.active == 1`): `Up`/`Down`
/// scroll a line, `PageUp`/`PageDown` a page, `l` opens the link search.
/// `Esc`/`F3` close the whole session through
/// `editor::close_editor_or_confirm`, so unsaved edits still get the
/// discard prompt.
pub fn handle_markdown_edit_preview_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if let KeyCode::Char('l' | 'L') = key.code {
        open_link_search(app);
        return Ok(());
    }
    if matches!(key.code, KeyCode::Esc | KeyCode::F(3)) {
        return editor::close_editor_or_confirm(app);
    }

    let Some(preview) = &mut app.markdown_edit_preview else {
        return Ok(());
    };
    match key.code {
        KeyCode::Up => preview.scroll_up(),
        KeyCode::Down => preview.scroll_down(),
        KeyCode::PageUp => preview.page_up(),
        KeyCode::PageDown => preview.page_down(),
        _ => {}
    }
    Ok(())
}


/// `l`: the keyboard link list (`Overlay::MarkdownLinkSearch`) -- exact,
/// built from the parsed links rather than a click position. No-op
/// without links. History: docs/history/markdown-preview.md.
fn open_link_search(app: &mut App) {
    if !matches!(&app.mode, Mode::Editing(_)) {
        return;
    }
    let Some(preview) = &app.markdown_edit_preview else {
        return;
    };
    let links = preview.links();
    if links.is_empty() {
        return;
    }

    app.overlay = Some(Overlay::MarkdownLinkSearch(MarkdownLinkSearchState::new(links)));
}


/// Key handling on `Overlay::MarkdownLinkSearch`: typing filters the
/// list, `Up`/`Down` move within it, `Enter` opens the highlighted link
/// (`links::open_link`, against `App::markdown_edit_preview`) and
/// closes, `Esc` just closes.
pub fn handle_markdown_link_search_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        if matches!(app.overlay, Some(Overlay::MarkdownLinkSearch(_))) {
            app.overlay = None;
        }
        return;
    }

    if key.code == KeyCode::Enter {
        let Some(Overlay::MarkdownLinkSearch(search)) = &app.overlay else {
            return;
        };
        let selected = search.selected_link();
        app.overlay = None;
        if let Some(link) = selected {
            if let Some(preview) = &mut app.markdown_edit_preview {
                open_link(preview, &link.url);
            }
        }
        return;
    }

    let Some(Overlay::MarkdownLinkSearch(search)) = &mut app.overlay else {
        return;
    };
    match key.code {
        KeyCode::Up => search.move_up(),
        KeyCode::Down => search.move_down(),
        KeyCode::Backspace => search.pop_char(),
        KeyCode::Char(c) => search.push_char(c),
        _ => {}
    }
}


/// Mouse in an editor session with a linked preview: `Ctrl`+left-click on
/// a link opens it (via `links::resolve_link_target`), whichever half has
/// focus -- `Ctrl` so a plain click stays the terminal's. The wheel
/// scrolls the preview. Everything else is ignored.
pub fn handle_markdown_preview_mouse(app: &mut App, mouse: MouseEvent) {
    if !matches!(&app.mode, Mode::Editing(_)) {
        return;
    }
    let Some(state) = &mut app.markdown_edit_preview else {
        return;
    };

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if mouse.modifiers.contains(KeyModifiers::CONTROL) => {
            debug!(column = mouse.column, row = mouse.row, "markdown preview: ctrl+click");
            let Some(url) = state.link_at(mouse.column, mouse.row) else {
                debug!("markdown preview: ctrl+click landed on a line with no link");
                state.set_link_message("Ctrl+click: no link on this line (try 'l' to search links instead)");
                return;
            };
            let url = url.to_string();
            open_link(state, &url);
        }
        MouseEventKind::ScrollDown => state.scroll_down(),
        MouseEventKind::ScrollUp => state.scroll_up(),
        _ => {}
    }
}
