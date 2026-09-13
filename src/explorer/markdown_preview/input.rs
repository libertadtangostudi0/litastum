use crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::execute;
use tracing::{debug, warn};

use crate::app::{App, Mode};

use super::links::{open_link, MarkdownLinkSearchState};
use super::state::MarkdownPreviewState;

/// `F3`: opens `Mode::MarkdownPreview` for the active panel's own
/// selected entry, if it's a `.md`/`.markdown` file -- a silent no-op
/// otherwise. Switches the *right* panel to active (`app.active = 1`),
/// same convention `image_preview::open_preview` already established
/// for `F3`: that's the panel whose own area now shows the preview
/// instead of a file listing.
///
/// Also turns on mouse capture (`EnableMouseCapture`) -- requested
/// directly, so a link can be opened with a click (touchpad or mouse),
/// not just read. Scoped to exactly this preview session rather than
/// enabled for the whole app: mouse capture takes over the terminal's
/// own native text selection, which would otherwise get in the way of
/// copying paths/output with the mouse everywhere else in this file
/// manager -- `handle_markdown_preview_key` turns it back off the
/// moment the preview closes. A failed `execute!` (a real write error
/// to stdout) is logged, not surfaced -- same "never blocks on this"
/// rule as every other terminal-control call in this codebase; the
/// preview still opens either way, just without clickable links.
pub fn open_preview(app: &mut App) {
    let panel = app.active_panel();
    let Some(path) = panel.selected_path() else {
        return;
    };

    let Some(state) = MarkdownPreviewState::open(&path) else {
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

    app.active = 1;
    app.mode = Mode::MarkdownPreview(state);
}


/// Key handling while `Mode::MarkdownPreview` is showing: `Up`/`Down`
/// scroll one line, `PageUp`/`PageDown` a fixed chunk (`PAGE_SIZE`);
/// `l` opens the keyboard-driven link search (`Mode::MarkdownLinkSearch`,
/// `open_link_search`'s own doc comment); `Esc` or `F3` again closes
/// back to `Mode::Browsing` -- and turns mouse capture back off, if
/// `open_preview` actually turned it on (`app.mouse_capture_enabled` --
/// own doc comment on `App` explains why this is checked rather than
/// assumed).
pub fn handle_markdown_preview_key(app: &mut App, key: KeyEvent) {
    if let KeyCode::Char('l' | 'L') = key.code {
        open_link_search(app);
        return;
    }

    let Mode::MarkdownPreview(state) = &mut app.mode else {
        return;
    };

    match key.code {
        KeyCode::Up => state.scroll_up(),
        KeyCode::Down => state.scroll_down(),
        KeyCode::PageUp => state.page_up(),
        KeyCode::PageDown => state.page_down(),
        KeyCode::Esc | KeyCode::F(3) => {
            if app.mouse_capture_enabled {
                if let Err(err) = execute!(std::io::stdout(), DisableMouseCapture) {
                    warn!(%err, "failed to disable mouse capture after closing the markdown preview");
                }
                app.mouse_capture_enabled = false;
            }
            app.mode = Mode::Browsing;
        }
        _ => {}
    }
}


/// `l` on the Markdown preview: opens the keyboard-driven link browser
/// (`Mode::MarkdownLinkSearch`), requested
/// directly as a more reliable alternative to `Ctrl`+click
/// (`MarkdownPreviewState::link_at`'s own doc comment on why that's
/// only ever an approximation once word-wrap is involved) -- exact by
/// construction, since it works from the same parsed `MarkdownLink`
/// list `links::resolve_link_target` already trusts, not from guessing
/// which on-screen row a click landed on. A no-op if the document has
/// no links at all (nothing to search).
fn open_link_search(app: &mut App) {
    if !matches!(&app.mode, Mode::MarkdownPreview(_)) {
        return;
    }
    let Mode::MarkdownPreview(state) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::MarkdownPreview above");
    };

    let links = state.links();
    if links.is_empty() {
        app.mode = Mode::MarkdownPreview(state);
        return;
    }

    app.mode = Mode::MarkdownLinkSearch(state, MarkdownLinkSearchState::new(links));
}


/// Key handling on `Mode::MarkdownLinkSearch`: typing filters the list,
/// `Up`/`Down` move within it, `Enter` opens the highlighted link
/// (`links::open_link`) and returns to `Mode::MarkdownPreview`, `Esc`
/// cancels back to it unchanged.
pub fn handle_markdown_link_search_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        let Mode::MarkdownLinkSearch(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            return;
        };
        app.mode = Mode::MarkdownPreview(state);
        return;
    }

    if key.code == KeyCode::Enter {
        let Mode::MarkdownLinkSearch(_, search) = &app.mode else {
            return;
        };
        let selected = search.selected_link();
        let Mode::MarkdownLinkSearch(mut state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            unreachable!("just matched Mode::MarkdownLinkSearch above");
        };
        if let Some(link) = selected {
            open_link(&mut state, &link.url);
        }
        app.mode = Mode::MarkdownPreview(state);
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


/// Mouse handling while `Mode::MarkdownPreview` is showing:
/// `Ctrl`+left-click on a rendered line containing a link opens it with
/// the OS's own default handler (`system_open::open`/`open_url` --
/// built for opening a file/directory in the OS file manager or a URL
/// in the default browser respectively) -- but only once
/// `links::resolve_link_target` has actually turned the link's raw text
/// into something worth opening (see its own doc comment for the real
/// bug this guards against). Requires `Ctrl` (rather than a plain
/// click) so an ordinary click/drag can still be used for the
/// terminal's own purposes without every click on a link line firing
/// navigation -- the same convention most GUI terminals and editors use
/// for clickable links (VS Code's integrated terminal, iTerm2, ...). The
/// scroll wheel also scrolls the preview, same step as `Up`/`Down`,
/// with no `Ctrl` needed -- a natural, essentially-free addition once
/// mouse events were flowing through at all for the click-a-link
/// feature actually requested. Anything else (a plain click, a
/// right/middle click, mouse movement, drag) is ignored.
pub fn handle_markdown_preview_mouse(app: &mut App, mouse: MouseEvent) {
    let Mode::MarkdownPreview(state) = &mut app.mode else {
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
