//! `Alt+F1`/`Alt+F2` — Far Manager's own per-panel "change drive"
//! popup: `Alt+F1` always targets the *left* panel, `Alt+F2` always
//! the *right* one, regardless of which panel currently has focus
//! (`command_line/browsing.rs`'s own special case, same reasoning as
//! `Shift+F6`/`Alt+F7` — needs the raw `Alt` modifier).

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};

/// One drive/root a panel could be pointed at.
pub struct DriveInfo {
    /// Absolute root to navigate a panel to on `Enter` — fed straight
    /// into `Panel::change_dir` (its `Path::join` already replaces the
    /// whole path for an absolute target, so no separate method is
    /// needed just for this).
    pub root: String,
    /// The popup's leftmost column — `"C:"` on Windows, `"/"` on the
    /// Unix fallback below.
    pub label: String,
    pub kind: &'static str,
    /// `None` if the OS call failed (e.g. an empty removable/CD drive)
    /// — still shown, just without a size, matching real Far Manager
    /// rather than silently dropping the entry.
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
}

/// State for the `Alt+F1`/`Alt+F2` popup — which row is highlighted,
/// and which panel `Enter` navigates.
pub struct DriveMenu {
    pub drives: Vec<DriveInfo>,
    pub selected: usize,
    /// 0 (left) for `Alt+F1`, 1 (right) for `Alt+F2` — fixed at open
    /// time, independent of `App::active`.
    pub target_panel: usize,
}

impl DriveMenu {
    pub fn open(target_panel: usize) -> Self {
        Self { drives: enumerate_drives(), selected: 0, target_panel }
    }
}

#[cfg(windows)]
fn enumerate_drives() -> Vec<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    };

    let bitmask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();

    for i in 0..26u32 {
        if bitmask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();

        let kind = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
            DRIVE_FIXED => "fixed",
            DRIVE_REMOVABLE => "removable",
            DRIVE_REMOTE => "remote",
            DRIVE_CDROM => "cdrom",
            DRIVE_RAMDISK => "ramdisk",
            _ => "unknown",
        };

        let mut free_available = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_available, &mut total, &mut total_free) } != 0;

        drives.push(DriveInfo {
            root,
            label: format!("{letter}:"),
            kind,
            total_bytes: ok.then_some(total),
            free_bytes: ok.then_some(total_free),
        });
    }

    drives
}

/// Deliberately minimal — a single entry for the root filesystem, not
/// real mount-point enumeration. A Unix equivalent of "drives" is
/// closer to mount points than drive letters, and properly listing
/// those is real, separate work (same scope cut as `shell.rs`'s Unix
/// fallback, which also settles for a single built-in profile rather
/// than detecting what's actually installed).
#[cfg(not(windows))]
fn enumerate_drives() -> Vec<DriveInfo> {
    vec![DriveInfo {
        root: "/".to_string(),
        label: "/".to_string(),
        kind: "fixed",
        total_bytes: None,
        free_bytes: None,
    }]
}

/// Unit-suffixed size for the popup's total/free columns — GiB down
/// to MiB, one decimal place (`"254.3 G"`, `"13.8 M"`).
/// Pure and OS-independent, unlike `enumerate_drives` itself, so it
/// gets real test coverage without depending on the machine's actual
/// drives.
pub fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} G", bytes / GIB)
    } else {
        format!("{:.1} M", bytes / MIB)
    }
}

/// Key handling on the `Alt+F1`/`Alt+F2` popup: `Up`/`Down` move,
/// typing a letter jumps straight to the first drive whose label
/// starts with it *and* selects it, same as pressing `Enter` on it
/// (Far Manager's own convention — `C` picks `C:` in one keystroke,
/// no need to arrow down to it first and press `Enter` separately),
/// `Enter` itself navigates `target_panel` to the highlighted drive's
/// root and closes, `Esc` cancels with nothing touched.
pub fn handle_drive_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ChangeDrive(menu) = &mut app.mode else {
        return Ok(());
    };

    match key.code {
        KeyCode::Up => {
            menu.selected = menu.selected.saturating_sub(1);
            Ok(())
        }
        KeyCode::Down => {
            if menu.selected + 1 < menu.drives.len() {
                menu.selected += 1;
            }
            Ok(())
        }
        KeyCode::Enter => {
            let index = menu.selected;
            select_drive(app, index)
        }
        KeyCode::Esc => {
            app.mode = Mode::Browsing;
            Ok(())
        }
        KeyCode::Char(c) => {
            let starts_with_c = |drive: &DriveInfo| drive.label.chars().next().is_some_and(|first| first.eq_ignore_ascii_case(&c));
            match menu.drives.iter().position(starts_with_c) {
                Some(index) => select_drive(app, index),
                None => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

/// Navigates `Mode::ChangeDrive`'s `target_panel` to `drives[index]`'s
/// root and closes the popup — shared by `Enter` and by typing a
/// letter that matches a drive directly.
fn select_drive(app: &mut App, index: usize) -> Result<()> {
    let Mode::ChangeDrive(menu) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("only called while Mode::ChangeDrive is active");
    };
    if let Some(drive) = menu.drives.get(index) {
        debug!(root = %drive.root, panel = menu.target_panel, "change drive");
        app.panels[menu.target_panel].change_dir(&drive.root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    #[test]
    fn format_bytes_examples() {
        assert_eq!(format_bytes(254 * 1024 * 1024 * 1024), "254.0 G");
        assert_eq!(format_bytes((13.8 * 1024.0 * 1024.0 * 1024.0) as u64), "13.8 G");
        assert_eq!(format_bytes(512 * 1024 * 1024), "512.0 M");
        assert_eq!(format_bytes(0), "0.0 M");
    }

    fn drives(labels: &[&str]) -> Vec<DriveInfo> {
        labels
            .iter()
            .map(|label| DriveInfo {
                root: format!("{label}\\"),
                label: label.to_string(),
                kind: "fixed",
                total_bytes: Some(100),
                free_bytes: Some(50),
            })
            .collect()
    }

    fn app_with_drive_menu(target_panel: usize, labels: &[&str]) -> App {
        let mut app = test_app(unique_scratch_dir("drive-menu"));
        app.mode = Mode::ChangeDrive(DriveMenu { drives: drives(labels), selected: 0, target_panel });
        app
    }

    #[test]
    fn down_is_clamped_at_the_last_drive() {
        let mut app = app_with_drive_menu(0, &["C:", "D:"]);
        for _ in 0..3 {
            handle_drive_menu_key(&mut app, key(KeyCode::Down)).unwrap();
        }
        let Mode::ChangeDrive(menu) = &app.mode else { panic!("expected Mode::ChangeDrive") };
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn typing_a_letter_selects_the_matching_drive_immediately() {
        // Typing a letter isn't just a jump-to-row cursor move -- it
        // should act exactly like pressing Enter on that row (Far
        // Manager's own convention), so this uses real scratch
        // directories as the "drive" roots and checks the panel
        // actually navigated and the popup closed, not just that
        // `selected` changed.
        let mut app = test_app(unique_scratch_dir("drive-menu-letter"));
        let c_root = unique_scratch_dir("drive-menu-letter-c");
        let w_root = unique_scratch_dir("drive-menu-letter-w");
        app.mode = Mode::ChangeDrive(DriveMenu {
            drives: vec![
                DriveInfo { root: c_root.display().to_string(), label: "C:".to_string(), kind: "fixed", total_bytes: None, free_bytes: None },
                DriveInfo { root: w_root.display().to_string(), label: "W:".to_string(), kind: "fixed", total_bytes: None, free_bytes: None },
            ],
            selected: 0,
            target_panel: 0,
        });

        handle_drive_menu_key(&mut app, key(KeyCode::Char('w'))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing), "typing a matching letter should close the popup, not just move the cursor");
        assert_eq!(app.panels[0].path, w_root, "lowercase 'w' should match and select 'W:' case-insensitively");
    }

    #[test]
    fn typing_an_unmatched_letter_leaves_the_selection_untouched() {
        let mut app = app_with_drive_menu(0, &["C:", "G:", "W:"]);

        handle_drive_menu_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        let Mode::ChangeDrive(menu) = &app.mode else { panic!("expected Mode::ChangeDrive") };
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn up_is_clamped_at_zero() {
        let mut app = app_with_drive_menu(0, &["C:", "D:"]);
        handle_drive_menu_key(&mut app, key(KeyCode::Up)).unwrap();
        let Mode::ChangeDrive(menu) = &app.mode else { panic!("expected Mode::ChangeDrive") };
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn esc_cancels_without_navigating() {
        let mut app = app_with_drive_menu(0, &["C:"]);
        let original_path = app.panels[0].path.clone();

        handle_drive_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert_eq!(app.panels[0].path, original_path);
    }

    #[test]
    fn enter_targets_the_menus_own_panel_not_the_active_one() {
        // Real Far behavior: Alt+F1 always means the left panel, even
        // if the right panel is the one with focus -- this is why
        // DriveMenu carries its own target_panel instead of reading
        // App::active at Enter time. Uses a real scratch directory
        // (not a Windows drive letter) as the "drive" root so this
        // passes deterministically regardless of platform or which
        // drive letters the test machine actually has.
        let mut app = test_app(unique_scratch_dir("drive-menu-target"));
        let target_dir = unique_scratch_dir("drive-menu-real-root");
        app.mode = Mode::ChangeDrive(DriveMenu {
            drives: vec![DriveInfo {
                root: target_dir.display().to_string(),
                label: "X:".to_string(),
                kind: "fixed",
                total_bytes: None,
                free_bytes: None,
            }],
            selected: 0,
            target_panel: 1, // the right panel
        });
        app.active = 0; // left panel has focus
        let original_left_path = app.panels[0].path.clone();

        handle_drive_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert_eq!(app.panels[1].path, target_dir, "target_panel (right) should have navigated");
        assert_eq!(app.panels[0].path, original_left_path, "the merely-active left panel should be untouched");
    }

    #[test]
    fn handle_drive_menu_key_is_a_noop_outside_change_drive_mode() {
        let mut app = app_with_drive_menu(0, &["C:"]);
        app.mode = Mode::Browsing;

        handle_drive_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
