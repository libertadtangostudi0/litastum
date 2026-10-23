use std::io;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::explorer::{execute, Command};
use crate::notice::Notice;
use crate::path_edit::{PathEdit, PathEditKey};

/// `Ctrl+L`/`Ctrl+F2`: the active panel's path title becomes a field, as
/// in Compare, the path selected (requested): typing replaces it,
/// `Ctrl+C` copies it, `End` or an arrow goes on from it.
pub(super) fn start(app: &mut App) {
    let mut edit = PathEdit::new(&app.panels[app.active].path);
    edit.field.select_all();
    app.panel_path_edit = Some(edit);
}


/// A click on the path title: as `start`, but nothing selected -- the
/// click counts as the first of a double click, which selects it all.
pub(super) fn start_by_click(app: &mut App) {
    app.panel_path_edit = Some(PathEdit::new(&app.panels[app.active].path).opened_by_click());
}


/// A mouse event in the browser, past an open path field's own
/// (`handle_browsing_mouse`, `PathEdit::mouse`): a left click on a
/// panel's title makes that panel active and edits its path, as in
/// Compare; a click anywhere else puts the title back. Everything else is
/// ignored.
pub fn mouse(app: &mut App, mouse: MouseEvent) {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    let on_title = app.panels.iter().position(|panel| {
        let area = panel.screen_area;
        mouse.row == area.y && mouse.column > area.x && mouse.column + 1 < area.x + area.width
    });
    match on_title {
        Some(index) => {
            app.active = index;
            start_by_click(app);
        }
        None => cancel(app),
    }
}


/// A key while the field is open. While typing, the panel shows the
/// directory the typed path is in (`follow`). `Enter` goes to the typed
/// directory; on a file it goes to the file's directory, selects it and
/// opens it in the editor (`edit_file`; requested), as `F4` and pasting a
/// file's path do (`pasted`). A path that isn't there keeps the field open
/// to fix it, with a notice. `Esc` takes the panel back to where it was.
/// `Down`/`Up` on a typed directory go on in the panel (`into_panel`).
pub(super) fn key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::F(4) && key.modifiers == KeyModifiers::NONE {
        edit_file(app);
        return;
    }
    if matches!(key.code, KeyCode::Down | KeyCode::Up) && key.modifiers == KeyModifiers::NONE && into_panel(app, key.code) {
        return;
    }
    let Some(edit) = app.panel_path_edit.as_mut() else {
        return;
    };
    match edit.key(key) {
        PathEditKey::Editing if is_paste_key(key) => pasted(app),
        PathEditKey::Editing => follow(app),
        PathEditKey::Cancel => cancel(app),
        PathEditKey::Submit(path) if edit.base().join(&path).is_file() => edit_file(app),
        PathEditKey::Submit(path) => match go_to(app, &path) {
            Ok(()) => app.panel_path_edit = None,
            Err(err) => app.notice = Some(Notice::error(format!("Can't open {}: {err}", path.display()))),
        },
    }
}


/// Reported: completing `dist\` left the panel where it was. The panel
/// now shows the typed path if it's a directory, else the directory it's
/// in (`...\dist\pa` -> `dist`), once that exists; a path that leads
/// nowhere leaves it alone.
fn follow(app: &mut App) {
    let Some(edit) = &app.panel_path_edit else {
        return;
    };
    let typed = edit.base().join(edit.field.text().trim().trim_matches('"'));
    let shown = if typed.is_dir() { Some(typed.as_path()) } else { typed.parent().filter(|dir| dir.is_dir()) };
    let Some(shown) = shown else {
        return;
    };
    let panel = &mut app.panels[app.active];
    if panel.path != shown {
        // Unreadable: the panel stays, Enter will say why.
        let _ = panel.change_dir(&shown.to_string_lossy());
    }
}


/// `F4` on a file highlighted in the `Tab` list, or typed out in full: the
/// panel goes to its directory with the file under the cursor, the field
/// closes, and the editor opens the file -- as `F4` on the panel would.
/// Anything else (a directory, nothing matching) is ignored.
fn edit_file(app: &mut App) {
    let Some(edit) = &app.panel_path_edit else {
        return;
    };
    let typed = match &edit.completions {
        Some(list) => list.accepted(),
        None => Some(edit.field.text().trim().trim_matches('"').to_string()),
    };
    let Some(path) = typed.map(|typed| edit.base().join(typed)).filter(|path| path.is_file()) else {
        return;
    };
    if let Err(err) = go_to(app, &path) {
        app.notice = Some(Notice::error(format!("Can't open {}: {err}", path.display())));
        return;
    }
    app.panel_path_edit = None;
    if let Err(err) = execute(Command::EditSelected, app) {
        app.notice = Some(Notice::error(format!("Can't edit {}: {err}", path.display())));
    }
}


/// Requested: with a directory typed out and no `Tab` list open, `Down`
/// (or `Up`) leaves the field for the panel, which already shows that
/// directory, and moves its cursor. `false` -- the key is the field's --
/// for anything else: a partial name, a file, an open list.
fn into_panel(app: &mut App, code: KeyCode) -> bool {
    let Some(edit) = &app.panel_path_edit else {
        return false;
    };
    // With the panels hidden there's no panel to go on in.
    if edit.completions.is_some() || app.panels_hidden {
        return false;
    }
    let typed = edit.field.text().trim().trim_matches('"');
    if typed.is_empty() || !edit.base().join(typed).is_dir() {
        return false;
    }
    let path = std::path::PathBuf::from(typed);
    if let Err(err) = go_to(app, &path) {
        app.notice = Some(Notice::error(format!("Can't open {}: {err}", path.display())));
        return true;
    }
    app.panel_path_edit = None;
    let command = if code == KeyCode::Down { Command::MoveDown } else { Command::MoveUp };
    if let Err(err) = execute(command, app) {
        app.notice = Some(Notice::error(err.to_string()));
    }
    true
}


/// The field's own paste key; in Windows Terminal `Ctrl+V` comes as the
/// clipboard typed in instead (`event_loop::paste`), which calls `pasted`.
fn is_paste_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('v' | 'V')) && key.modifiers.contains(KeyModifiers::CONTROL)
}


/// After a paste into the field (requested): a path to an existing file
/// goes to its directory, selects it and opens it in the editor, as `F4`
/// would -- pasting a file's path means opening it. Anything else (a
/// directory, a partial path) stays typed, the panel following it.
pub(crate) fn pasted(app: &mut App) {
    let Some(edit) = app.panel_path_edit.as_mut() else {
        return;
    };
    // A paste typed into an open `Tab` list's filter is for the field.
    edit.completions = None;
    let typed = edit.base().join(edit.field.text().trim().trim_matches('"'));
    if typed.is_file() {
        edit_file(app);
    } else {
        follow(app);
    }
}


/// `Esc` or a click elsewhere: the title comes back, and the panel goes
/// back to the directory it had before the field followed the typing.
pub(super) fn cancel(app: &mut App) {
    let Some(edit) = app.panel_path_edit.take() else {
        return;
    };
    let panel = &mut app.panels[app.active];
    if panel.path != edit.base() {
        let _ = panel.change_dir(&edit.base().to_string_lossy());
    }
}


/// Relative paths are taken from the directory the field started in.
fn go_to(app: &mut App, path: &Path) -> io::Result<()> {
    let base = app.panel_path_edit.as_ref().map(|edit| edit.base().to_path_buf()).unwrap_or_default();
    let panel = &mut app.panels[app.active];
    let full = base.join(path);
    if full.is_file() {
        panel.reveal(&full)?;
        return Ok(());
    }
    if panel.change_dir(&full.to_string_lossy())? {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "no such directory"))
    }
}


#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::KeyCode;

    use super::super::handle_browsing_key;
    use crate::test_support::{ctrl_key, key, test_app, unique_scratch_dir};

    fn type_path(app: &mut crate::app::App, path: &str) {
        app.panel_path_edit.as_mut().unwrap().field.set_text(path);
    }

    #[test]
    fn ctrl_l_opens_the_field_with_the_panel_path() {
        let dir = unique_scratch_dir("panel-path");
        let mut app = test_app(dir.clone());

        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();

        assert_eq!(app.panel_path_edit.as_ref().unwrap().field.text(), dir.to_string_lossy());
    }

    /// Requested: `Ctrl+F2` selects the whole path -- typing replaces it,
    /// `End` goes on from it.
    #[test]
    fn ctrl_f2_opens_with_the_path_selected() {
        let dir = unique_scratch_dir("panel-path");
        let mut app = test_app(dir.clone());

        handle_browsing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::F(2), crossterm::event::KeyModifiers::CONTROL)).unwrap();
        let len = dir.to_string_lossy().chars().count();
        assert_eq!(app.panel_path_edit.as_ref().unwrap().field.selection(), Some((0, len)));

        handle_browsing_key(&mut app, key(KeyCode::Char('C'))).unwrap();
        assert_eq!(app.panel_path_edit.as_ref().unwrap().field.text(), "C", "typing replaced the path");
    }

    #[test]
    fn typing_goes_into_the_field_not_the_command_line() {
        let mut app = test_app(unique_scratch_dir("panel-path"));
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();

        handle_browsing_key(&mut app, key(KeyCode::Char('x'))).unwrap();

        assert!(app.command_line.is_empty());
        assert!(app.panel_path_edit.as_ref().unwrap().field.text().ends_with('x'));
    }

    #[test]
    fn enter_on_a_directory_moves_the_panel_there() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("sub")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &dir.join("sub").to_string_lossy());

        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.panels[app.active].path, dir.join("sub"));
        assert!(app.panel_path_edit.is_none());
    }

    /// Requested: a file's path typed by hand opens it too, as a pasted one.
    #[test]
    fn enter_on_a_file_selects_it_in_its_directory_and_opens_the_editor() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("b.txt"), "b").unwrap();
        fs::write(dir.join("sub").join("a.txt"), "a").unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, "sub/b.txt");

        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.panel_path_edit.is_none());
        let panel = &app.panels[app.active];
        assert_eq!(panel.path, dir.join("sub"));
        assert_eq!(panel.current().map(|entry| entry.name.as_str()), Some("b.txt"));
        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("sub").join("b.txt"));
    }

    #[test]
    fn a_missing_path_keeps_the_field_open_with_a_notice() {
        let dir = unique_scratch_dir("panel-path");
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, "no/such/dir");

        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.panel_path_edit.is_some());
        assert!(app.notice.is_some());
        assert_eq!(app.panels[app.active].path, dir);
    }

    fn click(column: u16, row: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    /// Reported: clicking a panel's path did nothing, unlike Compare's.
    #[test]
    fn a_click_on_a_panel_title_edits_that_panels_path() {
        let dir = unique_scratch_dir("panel-path");
        let mut app = test_app(dir.clone());
        app.panels[0].screen_area = ratatui::layout::Rect::new(0, 0, 40, 20);
        app.panels[1].screen_area = ratatui::layout::Rect::new(40, 0, 40, 20);

        super::mouse(&mut app, click(50, 0));

        assert_eq!(app.active, 1, "the clicked panel becomes active");
        assert_eq!(app.panel_path_edit.as_ref().unwrap().field.text(), dir.to_string_lossy());

        super::mouse(&mut app, click(50, 5));
        assert!(app.panel_path_edit.is_none(), "a click elsewhere puts the title back");
    }

    #[test]
    fn a_click_on_a_border_corner_or_a_hidden_panel_does_nothing() {
        let mut app = test_app(unique_scratch_dir("panel-path"));
        app.panels[0].screen_area = ratatui::layout::Rect::new(0, 0, 40, 20);

        super::mouse(&mut app, click(0, 0));
        super::mouse(&mut app, click(39, 0));
        super::mouse(&mut app, click(50, 0)); // panel 1 isn't drawn: empty area

        assert!(app.panel_path_edit.is_none());
        assert_eq!(app.active, 0);
    }

    /// Reported: completing a directory didn't move the panel into it.
    #[test]
    fn the_panel_follows_the_typed_directory_and_esc_takes_it_back() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("dist").join("themes")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        handle_browsing_key(&mut app, key(KeyCode::End)).unwrap(); // past the selected path

        handle_browsing_key(&mut app, key(KeyCode::Char(std::path::MAIN_SEPARATOR))).unwrap();
        for c in "dist".chars() {
            handle_browsing_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        assert_eq!(app.panels[app.active].path, dir.join("dist"), "a whole directory name moves the panel");

        for c in [std::path::MAIN_SEPARATOR, 'p', 'a'] {
            handle_browsing_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        assert_eq!(app.panels[app.active].path, dir.join("dist"), "a partial name shows its directory");

        handle_browsing_key(&mut app, key(KeyCode::Esc)).unwrap();
        assert_eq!(app.panels[app.active].path, dir, "Esc goes back");
    }

    #[test]
    fn a_completed_directory_moves_the_panel() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("dist")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &format!("{}{}di", dir.display(), std::path::MAIN_SEPARATOR));

        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();

        assert_eq!(app.panels[app.active].path, dir.join("dist"));
    }

    /// The whole route through the browser's keys: a partial name, `Tab`,
    /// a pick from the list, and the panel following into it.
    #[test]
    fn a_partial_name_completes_from_the_list_and_the_panel_follows() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("packages")).unwrap();
        fs::create_dir_all(dir.join("packaging")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        handle_browsing_key(&mut app, key(KeyCode::End)).unwrap(); // past the selected path
        for c in [std::path::MAIN_SEPARATOR, 'p', 'a'] {
            handle_browsing_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }

        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        let list = app.panel_path_edit.as_ref().unwrap().completions.as_ref().expect("two matches open the list");
        assert_eq!(list.items.len(), 2);
        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap();
        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.panel_path_edit.as_ref().is_some_and(|edit| edit.completions.is_none()), "Enter picked, the field stays");
        assert_eq!(app.panels[app.active].path, dir.join("packaging"), "the panel followed");
        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();
        assert!(app.panel_path_edit.is_none());
        assert_eq!(app.panels[app.active].path, dir.join("packaging"));
    }

    /// Requested: F4 on a file in the list selects it in the panel and
    /// opens it in the editor.
    #[test]
    fn f4_on_a_file_in_the_list_selects_it_and_opens_the_editor() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs").join("notes.md"), "hello\n").unwrap();
        fs::write(dir.join("docs").join("news.txt"), "").unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &format!("{}{}docs{}n", dir.display(), std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR));
        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap(); // news.txt, then notes.md

        handle_browsing_key(&mut app, key(KeyCode::F(4))).unwrap();

        assert!(app.panel_path_edit.is_none());
        let panel = &app.panels[app.active];
        assert_eq!(panel.path, dir.join("docs"));
        assert_eq!(panel.current().map(|entry| entry.name.as_str()), Some("notes.md"));
        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("docs").join("notes.md"));
    }

    #[test]
    fn f4_on_a_directory_in_the_list_does_nothing() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::create_dir_all(dir.join("dist")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &format!("{}{}d", dir.display(), std::path::MAIN_SEPARATOR));
        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();

        handle_browsing_key(&mut app, key(KeyCode::F(4))).unwrap();

        assert!(app.panel_path_edit.is_some(), "the field and its list stay");
        assert!(matches!(app.mode, crate::app::Mode::Browsing));
    }

    /// Requested: pasting a file's path goes to its directory, selects it
    /// and opens it in the editor.
    #[test]
    fn pasting_a_file_path_selects_the_file_and_opens_the_editor() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs").join("notes.md"), "hello
").unwrap();
        fs::write(dir.join("docs").join("a.txt"), "").unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        let file = dir.join("docs").join("notes.md");
        type_path(&mut app, &format!("\"{}\"", file.display()));

        super::pasted(&mut app);

        assert!(app.panel_path_edit.is_none());
        let panel = &app.panels[app.active];
        assert_eq!(panel.path, dir.join("docs"));
        assert_eq!(panel.current().map(|entry| entry.name.as_str()), Some("notes.md"));
        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), file);
    }

    /// A directory's path, or one not finished yet, just stays typed.
    #[test]
    fn pasting_a_directory_path_only_follows_it() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("docs")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &dir.join("docs").to_string_lossy());

        super::pasted(&mut app);

        assert!(app.panel_path_edit.is_some(), "the field stays");
        assert_eq!(app.panels[app.active].path, dir.join("docs"));
        assert!(matches!(app.mode, crate::app::Mode::Browsing));
    }

    /// The field's own `Ctrl+V` (outside Windows Terminal) opens a pasted
    /// file too.
    #[test]
    fn ctrl_v_in_the_field_opens_a_pasted_file() {
        let dir = unique_scratch_dir("panel-path");
        fs::write(dir.join("notes.md"), "hello
").unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        app.panel_path_edit.as_mut().unwrap().field.select_all();
        crate::text_field::clipboard::set(dir.join("notes.md").to_string_lossy().into_owned());

        handle_browsing_key(&mut app, ctrl_key('v')).unwrap();

        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("notes.md"));
    }

    /// Requested: a directory typed out, `Down` goes on in its panel --
    /// the field closes and the panel's cursor moves.
    #[test]
    fn down_on_a_typed_directory_moves_on_in_its_panel() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs").join("a.txt"), "").unwrap();
        fs::write(dir.join("docs").join("b.txt"), "").unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &format!("{}{}docs{}", dir.display(), std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR));

        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(app.panel_path_edit.is_none(), "the field closed");
        let panel = &app.panels[app.active];
        assert_eq!(panel.path, dir.join("docs"));
        assert_eq!(panel.current().map(|entry| entry.name.as_str()), Some("a.txt"), "moved down off '..'");
    }

    /// A partial name isn't a directory yet: `Down` stays the field's.
    #[test]
    fn down_on_a_partial_name_stays_in_the_field() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("docs")).unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, &format!("{}{}do", dir.display(), std::path::MAIN_SEPARATOR));

        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(app.panel_path_edit.is_some());
    }

    /// Reported: dragging to the far left selected the frame's corner too
    /// -- the terminal's own selection was all there was. A drag selects
    /// the path only, past its left end from its first character.
    #[test]
    fn a_drag_in_the_field_selects_the_text_only() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = test_app(unique_scratch_dir("panel-path"));
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, r"W:\folder");
        let edit = app.panel_path_edit.as_ref().unwrap();
        edit.note_area(ratatui::layout::Rect::new(1, 0, 30, 1));
        edit.field.set_view_offset(0);
        let at = |kind, column| MouseEvent { kind, column, row: 0, modifiers: KeyModifiers::NONE };

        for (kind, column) in [(MouseEventKind::Down(MouseButton::Left), 10), (MouseEventKind::Drag(MouseButton::Left), 0), (MouseEventKind::Up(MouseButton::Left), 0)] {
            crate::command_line::handle_browsing_mouse(&mut app, at(kind, column));
        }

        let field = &app.panel_path_edit.as_ref().expect("the field stays open").field;
        assert_eq!(field.selection(), Some((0, 9)), "all of 'W:\\folder', nothing more");
    }

    #[test]
    fn a_click_in_the_field_places_the_caret() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = test_app(unique_scratch_dir("panel-path"));
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, r"W:\folder");
        let edit = app.panel_path_edit.as_ref().unwrap();
        edit.note_area(ratatui::layout::Rect::new(1, 0, 30, 1));
        edit.field.set_view_offset(0);

        crate::command_line::handle_browsing_mouse(&mut app, MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 4, row: 0, modifiers: KeyModifiers::NONE });

        let field = &app.panel_path_edit.as_ref().expect("a click in the field keeps it").field;
        assert_eq!((field.cursor(), field.selection()), (3, None));
    }

    /// Requested: a double click on a panel's path opens it as a field
    /// and selects it all -- quicker to copy.
    #[test]
    fn a_double_click_on_the_path_selects_all_of_it() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let dir = unique_scratch_dir("panel-path");
        let mut app = test_app(dir.clone());
        app.panels[0].screen_area = ratatui::layout::Rect::new(0, 0, 40, 10);
        let click = MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 5, row: 0, modifiers: KeyModifiers::NONE };

        crate::command_line::handle_browsing_mouse(&mut app, click);
        assert!(app.panel_path_edit.is_some(), "the first click opens the field");
        app.panel_path_edit.as_ref().unwrap().note_area(ratatui::layout::Rect::new(1, 0, 38, 1));
        crate::command_line::handle_browsing_mouse(&mut app, click);

        let field = &app.panel_path_edit.as_ref().unwrap().field;
        let len = dir.to_string_lossy().chars().count();
        assert_eq!(field.selection(), Some((0, len)), "the whole path selected");
    }

    #[test]
    fn esc_puts_the_title_back_and_leaves_the_panel_alone() {
        let dir = unique_scratch_dir("panel-path");
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, "elsewhere");

        handle_browsing_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.panel_path_edit.is_none());
        assert_eq!(app.panels[app.active].path, dir);
    }
}
