use color_eyre::eyre::Result;
use crossterm::event::{EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::execute;
use tracing::{debug, warn};

use crate::app::{App, Mode};
use crate::editor::{self, Editor};

use super::links::{open_link, MarkdownLinkSearchState};
use super::state::MarkdownPreviewState;

/// `F3` on a `.md`/`.markdown` file: opens the built-in editor
/// (`Editor::open`, same as `F4`) for it *and* a live rendered preview
/// side by side (`App::markdown_edit_preview`) -- requested directly
/// ("одновременно просматривать .md, редактировать его в левой панели
/// и при сохранении смотреть что в правой"). `app.active` starts at `0`
/// (the editor, ready to type into immediately); `Tab`
/// (`main.rs::handle_key_event`) toggles it to `1` (the preview) for
/// scrolling/`l`-searching/`Ctrl`+clicking it -- see
/// `handle_markdown_edit_preview_key`. A silent no-op if either half
/// fails to open (a corrupt/unreadable file, or one that's not valid
/// UTF-8 for the editor) -- same "couldn't act on this" convention
/// `explorer::command::open_editor` already uses for plain `F4`.
///
/// Also turns on mouse capture (`EnableMouseCapture`) -- requested
/// directly, so a link can be opened with a click (touchpad or mouse),
/// not just read. Scoped to exactly this session rather than enabled
/// for the whole app: mouse capture takes over the terminal's own
/// native text selection, which would otherwise get in the way of
/// copying paths/output with the mouse everywhere else in this file
/// manager -- `editor_keymap::return_from_editor` turns it back off the
/// moment the whole editor+preview session actually closes. A failed
/// `execute!` (a real write error to stdout) is logged, not surfaced --
/// same "never blocks on this" rule as every other terminal-control
/// call in this codebase; the session still opens either way, just
/// without clickable links.
pub fn open_edit_preview(app: &mut App) {
    let Some(path) = app.active_panel().selected_path() else {
        return;
    };

    let Some(preview) = MarkdownPreviewState::open(&path) else {
        return;
    };
    let syntax_theme = app.syntax_theme.clone();
    let Ok(editor) = Editor::open(path, syntax_theme, app.editor_keymap_mode) else {
        return;
    };

    match execute!(std::io::stdout(), EnableMouseCapture) {
        // `app.mouse_capture_enabled` is what tells `main.rs::restore_terminal`
        // it's safe to send `DisableMouseCapture` at all when the app
        // exits -- only set once this actually succeeded, never
        // unconditionally (see its own doc comment for the crash that
        // came from assuming it was always safe).
        Ok(()) => app.mouse_capture_enabled = true,
        Err(err) => warn!(%err, "failed to enable mouse capture for the markdown preview"),
    }

    app.markdown_edit_preview = Some(preview);
    app.active = 0;
    app.mode = Mode::Editing(editor);
}


/// Key handling while `App::active == 1` -- the embedded preview has
/// focus, not the editor -- during a `Mode::Editing`/`ConfirmDiscard`
/// session that has a linked `App::markdown_edit_preview`
/// (`main.rs::handle_key_event` is what routes here instead of
/// `editor::handle_editor_key`, based on `app.active`). `Up`/`Down`
/// scroll one line, `PageUp`/`PageDown` a fixed chunk (`PAGE_SIZE`);
/// `l` opens the keyboard-driven link search (`Mode::MarkdownLinkSearch`,
/// `open_link_search`'s own doc comment). `Esc`/`F3` close the *whole*
/// session, not just the preview half -- reusing
/// `editor::close_editor_or_confirm` so an unsaved editor buffer still
/// gets the same "discard changes?" prompt it would closing from the
/// editor side, regardless of which half had focus at the time.
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


/// `l` on the embedded preview: opens the keyboard-driven link browser
/// (`Mode::MarkdownLinkSearch`), requested directly as a more reliable
/// alternative to `Ctrl`+click (`MarkdownPreviewState::link_at`'s own
/// doc comment on why that's only ever an approximation once word-wrap
/// is involved) -- exact by construction, since it works from the same
/// parsed `MarkdownLink` list `links::resolve_link_target` already
/// trusts, not from guessing which on-screen row a click landed on.
/// A no-op if the document has no links at all (nothing to search).
/// "Parks" the `Editor` inside `Mode::MarkdownLinkSearch` itself --
/// `Mode` can only ever hold one thing at a time, so the editor can't
/// stay in `Mode::Editing` while this popup is up -- `App::markdown_edit_preview`
/// (the `MarkdownPreviewState` this list was built from) is untouched,
/// still sitting on `App` throughout.
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

    let Mode::Editing(editor) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::Editing above");
    };
    app.mode = Mode::MarkdownLinkSearch(editor, MarkdownLinkSearchState::new(links));
}


/// Key handling on `Mode::MarkdownLinkSearch`: typing filters the list,
/// `Up`/`Down` move within it, `Enter` opens the highlighted link
/// (`links::open_link`, against `App::markdown_edit_preview`) and
/// returns to `Mode::Editing` with the parked editor restored, `Esc`
/// cancels back to it unchanged.
pub fn handle_markdown_link_search_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        let Mode::MarkdownLinkSearch(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            return;
        };
        app.mode = Mode::Editing(editor);
        return;
    }

    if key.code == KeyCode::Enter {
        let Mode::MarkdownLinkSearch(_, search) = &app.mode else {
            return;
        };
        let selected = search.selected_link();
        let Mode::MarkdownLinkSearch(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            unreachable!("just matched Mode::MarkdownLinkSearch above");
        };
        if let Some(link) = selected {
            if let Some(preview) = &mut app.markdown_edit_preview {
                open_link(preview, &link.url);
            }
        }
        app.mode = Mode::Editing(editor);
        return;
    }

    let Mode::MarkdownLinkSearch(_, search) = &mut app.mode else {
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


/// Mouse handling during a `Mode::Editing` session with a linked
/// `App::markdown_edit_preview`: `Ctrl`+left-click on a rendered line
/// containing a link opens it with the OS's own default handler
/// (`system_open::open`/`open_url` -- built for opening a file/
/// directory in the OS file manager or a URL in the default browser
/// respectively) -- but only once `links::resolve_link_target` has
/// actually turned the link's raw text into something worth opening
/// (see its own doc comment for the real bug this guards against).
/// Requires `Ctrl` (rather than a plain click) so an ordinary
/// click/drag can still be used for the terminal's own purposes
/// without every click on a link line firing navigation -- the same
/// convention most GUI terminals and editors use for clickable links
/// (VS Code's integrated terminal, iTerm2, ...). Works regardless of
/// `App::active` -- a mouse click is a real screen position, not
/// something that needs the preview to already have keyboard focus.
/// The scroll wheel also scrolls the preview, same step as `Up`/`Down`,
/// with no `Ctrl` needed. Anything else (a plain click, a right/middle
/// click, mouse movement, drag) is ignored.
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
