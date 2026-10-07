use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::command_line::effect::Effect;
use crate::explorer::Command;
use crate::notice::Notice;
use crate::text_field::EditOutcome;

use super::bindings::BrowserAction;
use super::shell_exec::submit_command_line;

/// Lines the mouse wheel scrolls the user screen per tick.
const WHEEL_LINES: isize = 3;


/// Whether an already-normalized `key` is `Ctrl+O`.
fn is_ctrl_o(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)
}


/// A key while the panels are hidden (`Ctrl+O`, `App::panels_hidden`):
/// the user screen with our own command line, a real one as in Far.
/// `Ctrl+O` brings the panels back. The keys that mean something without
/// them work as over the panels: `F2` the user menu, `F9` the menu, `F10`
/// quit, `Alt+F7` Find file (from the active panel's directory), and
/// `Ctrl+F2`/`Ctrl+L` edit the path in the prompt, as the panels' title
/// (`panel_path`; the panel follows, unseen, and the prompt with it). While
/// suggestions show, `Up`/`Down`/`Tab`/`F4`/`F8`/`Esc` work on them;
/// `Enter` runs the line, its output joining the screen live, and stays;
/// `PageUp`/`PageDown` scroll; `Esc` clears the line; `Tab` completes a
/// path; the rest edits the line -- arrows included. History:
/// docs/history/command-execution.md.
pub(super) fn console_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    if is_ctrl_o(key) {
        app.panels_hidden = false;
        app.user_screen.clear_selection();
        app.active_panel().reload()?;
        return Ok(Effect::None);
    }
    // Text selected with the mouse: `Ctrl+C`/`Ctrl+Insert` copy it, `Esc`
    // drops it -- before they'd act on the command line.
    if let Some(text) = app.user_screen.selected_text() {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c' | 'C') | KeyCode::Insert if ctrl => {
                crate::text_field::clipboard::set(text);
                app.user_screen.clear_selection();
                app.notice = Some(Notice::info("Copied"));
                return Ok(Effect::None);
            }
            KeyCode::Esc => {
                app.user_screen.clear_selection();
                return Ok(Effect::None);
            }
            _ => {}
        }
    }
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let plain = !key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    // The path in the prompt as a field, as the panels' title is.
    if ctrl && matches!(key.code, KeyCode::F(2) | KeyCode::Char('l' | 'L')) {
        super::panel_path::start(app);
        return Ok(Effect::None);
    }
    let menu_action = match key.code {
        KeyCode::F(2) if plain => Some(BrowserAction::Navigate(Command::OpenUserMenu)),
        KeyCode::F(9) if plain => Some(BrowserAction::Navigate(Command::OpenMenu)),
        KeyCode::F(10) if plain => Some(BrowserAction::Navigate(Command::Quit)),
        KeyCode::F(7) if alt => Some(BrowserAction::OpenFindFile),
        _ => None,
    };
    if let Some(action) = menu_action {
        return super::perform(app, action);
    }

    if super::suggestions_showing(app) {
        let action = match key.code {
            KeyCode::Up => Some(BrowserAction::SuggestionUp),
            KeyCode::Down => Some(BrowserAction::SuggestionDown),
            KeyCode::Tab => Some(BrowserAction::AcceptSuggestion),
            KeyCode::F(4) => Some(BrowserAction::EditSuggestion),
            KeyCode::F(8) => Some(BrowserAction::DeleteSuggestion),
            KeyCode::Esc => {
                app.command_line_suggestion_dismissed = true;
                return Ok(Effect::None);
            }
            _ => None,
        };
        if let Some(action) = action {
            return super::perform(app, action);
        }
    }

    let rows = app.user_screen.visible_rows();
    match key.code {
        KeyCode::Enter => {
            app.command_line_completion = None;
            let effect = submit_command_line(app)?;
            super::line_edited(app);
            return Ok(effect);
        }
        KeyCode::PageUp => app.user_screen.scroll_by(rows as isize, rows),
        KeyCode::PageDown => app.user_screen.scroll_by(-(rows as isize), rows),
        KeyCode::Esc => {
            app.command_line.clear();
            super::line_edited(app);
        }
        KeyCode::Tab if !app.command_line.is_empty() => return super::perform(app, BrowserAction::Complete),
        _ => {
            if app.command_line.apply_key(key) == EditOutcome::TextChanged {
                super::line_edited(app);
            }
        }
    }
    Ok(Effect::None)
}


/// The mouse while the panels are hidden: a click on the prompt's path
/// edits it, as on a panel's title (a click elsewhere puts it back); a
/// drag selects text (scrolling on at the top and bottom rows), and so
/// does a click, then a `Shift`- or `Ctrl`-click at the other end, or a
/// double click (the word; again on it, the line) --
/// `Ctrl` because Windows Terminal keeps `Shift`-clicks for its own
/// selection, which can't scroll the alternate screen; the wheel scrolls
/// back.
pub(super) fn mouse(app: &mut App, mouse: MouseEvent) {
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
        // The command line is the row under the screen's lines.
        let on_prompt = usize::from(mouse.row) == app.user_screen.visible_rows() && usize::from(mouse.column) < app.panels[app.active].path.display().to_string().chars().count();
        if on_prompt {
            super::panel_path::start(app);
            return;
        }
        if app.panel_path_edit.is_some() {
            super::panel_path::cancel(app);
        }
    }
    let screen = &mut app.user_screen;
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => screen.click(mouse.row, mouse.column, mouse.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL), std::time::Instant::now()),
        MouseEventKind::Drag(MouseButton::Left) => screen.select_to(mouse.row, mouse.column),
        MouseEventKind::Up(MouseButton::Left) => screen.select_finish(),
        MouseEventKind::ScrollUp => screen.scroll_selecting(WHEEL_LINES, mouse.row, mouse.column),
        MouseEventKind::ScrollDown => screen.scroll_selecting(-WHEEL_LINES, mouse.row, mouse.column),
        _ => {}
    }
}


#[cfg(test)]
mod tests {
    use ratatui::text::Line;

    use super::*;
    use crate::app::Overlay;
    use crate::test_support::{ctrl_key, key, test_app, unique_scratch_dir};

    fn hidden(line: &str) -> App {
        let mut app = test_app(unique_scratch_dir("hidden-console"));
        app.panels_hidden = true;
        app.command_line.set_text(line);
        app
    }

    #[test]
    fn ctrl_o_brings_the_panels_back() {
        let mut app = hidden("");
        console_key(&mut app, ctrl_key('o')).unwrap();
        assert!(!app.panels_hidden);
    }

    /// Requested: F2, F9, F10 and Alt+F7 work with the panels hidden.
    #[test]
    fn the_menus_find_file_and_quit_work_without_the_panels() {
        let mut app = hidden("");
        console_key(&mut app, key(KeyCode::F(9))).unwrap();
        assert!(app.overlay.is_some(), "F9: the menu");

        let mut app = hidden("");
        console_key(&mut app, KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT)).unwrap();
        assert!(matches!(app.overlay, Some(Overlay::FindFile(_))), "Alt+F7: Find file");

        let mut app = hidden("");
        let menu = "[[item]]
title = \"status\"
hotkey = \"s\"
commands = [\"svn st\"]
";
        std::fs::write(app.panels[app.active].path.join("LitastumMenu.toml"), menu).unwrap();
        console_key(&mut app, key(KeyCode::F(2))).unwrap();
        assert!(matches!(app.overlay, Some(Overlay::UserMenu(_))), "F2: the user menu");

        let mut app = hidden("");
        console_key(&mut app, key(KeyCode::F(10))).unwrap();
        assert!(app.should_quit);
        assert!(app.panels_hidden, "the panels stay hidden meanwhile");
    }

    #[test]
    fn enter_hands_the_typed_line_over_to_run() {
        let mut app = hidden("svn st");

        assert_eq!(console_key(&mut app, key(KeyCode::Enter)).unwrap(), Effect::RunShell(vec!["svn st".to_string()]));
        assert!(app.command_line.is_empty());
        assert_eq!(app.command_history, ["svn st"]);
    }

    #[test]
    fn cls_asks_to_clear_the_screen() {
        let mut app = hidden("cls");
        assert_eq!(console_key(&mut app, key(KeyCode::Enter)).unwrap(), Effect::ClearScreen);
    }

    /// The suggestions work here as over the panels.
    #[test]
    fn suggestions_take_up_down_and_tab_and_esc_closes_them_first() {
        let mut app = hidden("svn");
        app.command_history = vec!["svn up".into(), "svn st".into()];

        console_key(&mut app, key(KeyCode::Down)).unwrap();
        console_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.command_line.text(), "svn up", "newest first: svn st, then svn up");

        let mut app = hidden("svn");
        app.command_history = vec!["svn up".into()];
        console_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert_eq!(app.command_line.text(), "svn", "the first Esc closes the list");
        console_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert!(app.command_line.is_empty(), "the next clears the line");
    }

    #[test]
    fn arrows_edit_the_line_and_page_keys_scroll() {
        let mut app = hidden("ac");
        console_key(&mut app, key(KeyCode::Left)).unwrap();
        console_key(&mut app, key(KeyCode::Char('b'))).unwrap();
        assert_eq!(app.command_line.text(), "abc");

        app.user_screen.extend((0..30).map(|n| Line::raw(n.to_string())).collect());
        app.user_screen.set_visible_rows(10);
        console_key(&mut app, key(KeyCode::PageUp)).unwrap();
        assert_eq!(app.user_screen.scroll(), 10);
        console_key(&mut app, key(KeyCode::PageDown)).unwrap();
        assert_eq!(app.user_screen.scroll(), 0);
    }

    /// Reported: text couldn't be selected on the user screen.
    #[test]
    fn a_mouse_drag_selects_and_ctrl_c_copies() {
        let mut app = hidden("typed");
        app.user_screen.extend(vec![Line::raw("first line"), Line::raw("second")]);
        app.user_screen.set_visible_rows(5);
        let at = |kind, column, row| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };

        mouse(&mut app, at(MouseEventKind::Down(MouseButton::Left), 6, 0));
        mouse(&mut app, at(MouseEventKind::Drag(MouseButton::Left), 2, 1));
        mouse(&mut app, at(MouseEventKind::Up(MouseButton::Left), 2, 1));
        console_key(&mut app, ctrl_key('c')).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("line\nsec"));
        assert_eq!(app.user_screen.selection(), None, "copied: the selection goes");
        assert_eq!(app.command_line.text(), "typed", "the command line untouched");
    }

    /// Requested: Ctrl+F2 edits the path here too, as over the panels.
    #[test]
    fn ctrl_f2_and_a_click_on_the_prompt_edit_the_path() {
        let mut app = hidden("");
        let dir = app.panels[app.active].path.clone();
        std::fs::create_dir_all(dir.join("sub")).unwrap();

        console_key(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.panel_path_edit.as_ref().unwrap().field.text(), dir.to_string_lossy());
        for c in [std::path::MAIN_SEPARATOR, 's', 'u', 'b'] {
            super::super::handle_browsing_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        super::super::handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();
        assert_eq!(app.panels[app.active].path, dir.join("sub"));
        assert!(app.panels_hidden, "still the user screen");

        app.user_screen.set_visible_rows(10);
        let click = |row| MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 2, row, modifiers: KeyModifiers::NONE };
        mouse(&mut app, click(10));
        assert!(app.panel_path_edit.is_some(), "a click on the prompt's path");
        mouse(&mut app, click(3));
        assert!(app.panel_path_edit.is_none(), "a click elsewhere puts it back");
    }

    /// Reported: a cd to a missing directory said nothing.
    #[test]
    fn a_cd_to_a_missing_directory_says_so_on_the_screen() {
        let mut app = hidden("cd no-such-dir");
        let dir = app.panels[app.active].path.clone();

        console_key(&mut app, key(KeyCode::Enter)).unwrap();

        let screen: Vec<String> = app.user_screen.lines().iter().map(|line| line.to_string()).collect();
        assert_eq!(screen, [format!("{}> cd no-such-dir", dir.display()), "cd: no such directory: no-such-dir".to_string()]);
        assert_eq!(app.panels[app.active].path, dir);
        assert!(app.notice.is_none(), "the screen shows it already");
    }
}
