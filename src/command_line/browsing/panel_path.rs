use std::io;
use std::path::Path;

use crossterm::event::KeyEvent;

use crate::app::App;
use crate::notice::Notice;
use crate::path_edit::{PathEdit, PathEditKey};

/// `Ctrl+L`: the active panel's path title becomes a field, as in Compare.
pub(super) fn start(app: &mut App) {
    app.panel_path_edit = Some(PathEdit::new(&app.panels[app.active].path));
}


/// A key while the field is open. `Enter` goes to the typed directory,
/// or to a typed file's directory with the file under the cursor; a path
/// that isn't there keeps the field open to fix it, with a notice.
pub(super) fn key(app: &mut App, key: KeyEvent) {
    let Some(edit) = app.panel_path_edit.as_mut() else {
        return;
    };
    match edit.key(key) {
        PathEditKey::Editing => {}
        PathEditKey::Cancel => app.panel_path_edit = None,
        PathEditKey::Submit(path) => match go_to(app, &path) {
            Ok(()) => app.panel_path_edit = None,
            Err(err) => app.notice = Some(Notice::error(format!("Can't open {}: {err}", path.display()))),
        },
    }
}


/// Relative paths are taken from the panel's current directory.
fn go_to(app: &mut App, path: &Path) -> io::Result<()> {
    let panel = &mut app.panels[app.active];
    let full = panel.path.join(path);
    if full.is_file() {
        let (Some(parent), Some(name)) = (full.parent(), full.file_name()) else {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        };
        panel.change_dir(&parent.to_string_lossy())?;
        if let Some(index) = panel.entries.iter().position(|entry| std::ffi::OsStr::new(&entry.name) == name) {
            panel.selected = index;
        }
        return Ok(());
    }
    if panel.change_dir(&path.to_string_lossy())? {
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

    #[test]
    fn enter_on_a_file_selects_it_in_its_directory() {
        let dir = unique_scratch_dir("panel-path");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("b.txt"), "b").unwrap();
        fs::write(dir.join("sub").join("a.txt"), "a").unwrap();
        let mut app = test_app(dir.clone());
        handle_browsing_key(&mut app, ctrl_key('l')).unwrap();
        type_path(&mut app, "sub/b.txt");

        handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        let panel = &app.panels[app.active];
        assert_eq!(panel.path, dir.join("sub"));
        assert_eq!(panel.current().map(|entry| entry.name.as_str()), Some("b.txt"));
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
