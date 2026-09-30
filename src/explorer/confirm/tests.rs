use super::*;
use crate::app::Mode;
use crate::app::{App, DeleteEntry, Overlay, PendingDelete, PendingTransfer, TransferSource};
use crate::test_support::{key, test_app, unique_scratch_dir};
use crate::text_field::TextField;

fn scratch_dir() -> PathBuf {
    unique_scratch_dir("confirm")
}

fn scratch_app() -> App {
    test_app(scratch_dir())
}

fn pending_delete(path: PathBuf, name: &str, is_dir: bool, size: u64) -> PendingDelete {
    PendingDelete { entries: vec![DeleteEntry { path, name: name.into(), is_dir, size }] }
}

#[test]
fn confirm_delete_removes_a_file_and_returns_to_browsing() {
    let mut app = scratch_app();
    let file = app.panels[0].path.join("victim.txt");
    fs::write(&file, b"bye").unwrap();
    app.overlay = Some(Overlay::ConfirmDelete(pending_delete(file.clone(), "victim.txt", false, 3)));

    handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

    assert!(!file.exists());
    assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
}

#[test]
fn confirm_delete_removes_a_directory_recursively() {
    let mut app = scratch_app();
    let dir = app.panels[0].path.join("victim_dir");
    fs::create_dir_all(dir.join("nested")).unwrap();
    fs::write(dir.join("nested").join("f.txt"), b"x").unwrap();
    app.overlay = Some(Overlay::ConfirmDelete(pending_delete(dir.clone(), "victim_dir", true, 0)));

    handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

    assert!(!dir.exists());
}

#[test]
fn confirm_delete_cancel_leaves_the_file_untouched() {
    let mut app = scratch_app();
    let file = app.panels[0].path.join("keep.txt");
    fs::write(&file, b"stay").unwrap();
    app.overlay = Some(Overlay::ConfirmDelete(pending_delete(file.clone(), "keep.txt", false, 4)));

    handle_confirm_delete_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(file.exists());
    assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
}

/// Regression coverage for the real request: F8 should delete every
/// marked entry, not just the one under the cursor.
#[test]
fn confirm_delete_removes_every_entry_in_a_multi_entry_prompt() {
    let mut app = scratch_app();
    let a = app.panels[0].path.join("a.txt");
    let b = app.panels[0].path.join("b.txt");
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"b").unwrap();
    app.overlay = Some(Overlay::ConfirmDelete(PendingDelete {
        entries: vec![
            DeleteEntry { path: a.clone(), name: "a.txt".into(), is_dir: false, size: 1 },
            DeleteEntry { path: b.clone(), name: "b.txt".into(), is_dir: false, size: 1 },
        ],
    }));

    handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

    assert!(!a.exists());
    assert!(!b.exists());
    assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
}

#[test]
fn confirm_delete_ignores_unrelated_keys_and_stays_open() {
    let mut app = scratch_app();
    let file = app.panels[0].path.join("keep2.txt");
    fs::write(&file, b"stay").unwrap();
    app.overlay = Some(Overlay::ConfirmDelete(pending_delete(file.clone(), "keep2.txt", false, 4)));

    handle_confirm_delete_key(&mut app, key(KeyCode::Char('x'))).unwrap();

    assert!(file.exists());
    assert!(matches!(app.overlay, Some(Overlay::ConfirmDelete(_))));
}

fn pending_transfer(operation: TransferOp, source: PathBuf, destination: String) -> PendingTransfer {
    PendingTransfer {
        operation,
        sources: vec![TransferSource { path: source, name: "irrelevant".into(), is_dir: false }],
        destination: TextField::with_text(destination),
    }
}

#[test]
fn confirm_transfer_enter_copies_and_keeps_the_source() {
    let mut app = scratch_app();
    let src = app.panels[0].path.join("source.txt");
    fs::write(&src, b"hello").unwrap();
    let dst = app.panels[0].path.join("dest.txt");
    app.overlay = Some(Overlay::ConfirmTransfer(pending_transfer(TransferOp::Copy, src.clone(), dst.to_string_lossy().into_owned())));

    handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

    assert!(src.exists(), "copy should leave the source alone");
    assert_eq!(fs::read(&dst).unwrap(), b"hello");
    assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
}

#[test]
fn confirm_transfer_enter_moves_and_removes_the_source() {
    let mut app = scratch_app();
    let src = app.panels[0].path.join("source.txt");
    fs::write(&src, b"hello").unwrap();
    let dst = app.panels[0].path.join("dest.txt");
    app.overlay = Some(Overlay::ConfirmTransfer(pending_transfer(TransferOp::Move, src.clone(), dst.to_string_lossy().into_owned())));

    handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

    assert!(!src.exists(), "move should remove the source");
    assert_eq!(fs::read(&dst).unwrap(), b"hello");
}

#[test]
fn confirm_transfer_esc_cancels_without_touching_the_filesystem() {
    let mut app = scratch_app();
    let src = app.panels[0].path.join("source.txt");
    fs::write(&src, b"hello").unwrap();
    let dst = app.panels[0].path.join("dest.txt");
    app.overlay = Some(Overlay::ConfirmTransfer(pending_transfer(TransferOp::Copy, src.clone(), dst.to_string_lossy().into_owned())));

    handle_confirm_transfer_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(src.exists());
    assert!(!dst.exists());
    assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
}

#[test]
fn confirm_transfer_typing_edits_the_destination_in_place() {
    let mut app = scratch_app();
    app.overlay = Some(Overlay::ConfirmTransfer(pending_transfer(TransferOp::Copy, PathBuf::from("src"), "dest".to_string())));

    handle_confirm_transfer_key(&mut app, key(KeyCode::Char('!'))).unwrap();

    let Some(Overlay::ConfirmTransfer(pending)) = &app.overlay else {
        panic!("should stay in ConfirmTransfer");
    };
    assert_eq!(pending.destination.text(), "dest!");
    assert_eq!(pending.destination.cursor(), 5);
}

#[test]
fn confirm_transfer_a_failed_move_still_returns_to_browsing_without_panicking() {
    let mut app = scratch_app();
    let missing_src = app.panels[0].path.join("does-not-exist.txt");
    let dst = app.panels[0].path.join("dest.txt");
    app.overlay = Some(Overlay::ConfirmTransfer(pending_transfer(TransferOp::Move, missing_src, dst.to_string_lossy().into_owned())));

    handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

    assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    assert!(!dst.exists());
    let Some(notice) = &app.notice else { panic!("a failed move shows a notice") };
    assert!(notice.text.starts_with("Move failed: irrelevant:"), "{notice:?}");
}

#[test]
fn a_failed_delete_shows_a_notice() {
    let mut app = scratch_app();
    let missing = app.panels[0].path.join("already-gone.txt");
    app.overlay = Some(Overlay::ConfirmDelete(pending_delete(missing, "already-gone.txt", false, 0)));

    handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

    let Some(notice) = &app.notice else { panic!("a failed delete shows a notice") };
    assert_eq!(notice.kind, crate::notice::NoticeKind::Error);
    assert!(notice.text.starts_with("Delete failed: already-gone.txt:"), "{notice:?}");
}

#[test]
fn a_successful_delete_shows_no_notice() {
    let mut app = scratch_app();
    let file = app.panels[0].path.join("victim.txt");
    fs::write(&file, b"bye").unwrap();
    app.overlay = Some(Overlay::ConfirmDelete(pending_delete(file, "victim.txt", false, 3)));

    handle_confirm_delete_key(&mut app, key(KeyCode::Char('y'))).unwrap();

    assert_eq!(app.notice, None);
}

#[test]
fn several_failures_are_counted() {
    let failures = vec![
        ("a.txt".to_string(), std::io::Error::other("locked")),
        ("b.txt".to_string(), std::io::Error::other("locked")),
    ];
    let notice = failure_notice("Delete", &failures, 5).unwrap();
    assert_eq!(notice.text, "Delete failed for 2 of 5 entries; first: a.txt: locked");
}

/// Regression coverage for the real request: F5 should copy every
/// marked entry, not just the one under the cursor -- several
/// `sources` join `destination` (a target *directory* here, not a
/// full file path) with each source's own name individually.
#[test]
fn confirm_transfer_enter_copies_every_marked_source_into_the_destination_directory() {
    let mut app = scratch_app();
    let src_dir = app.panels[0].path.clone();
    fs::write(src_dir.join("a.txt"), b"aaa").unwrap();
    fs::write(src_dir.join("b.txt"), b"bbb").unwrap();
    let dst_dir = app.panels[0].path.join("dest");
    fs::create_dir_all(&dst_dir).unwrap();
    app.overlay = Some(Overlay::ConfirmTransfer(PendingTransfer {
        operation: TransferOp::Copy,
        sources: vec![
            TransferSource { path: src_dir.join("a.txt"), name: "a.txt".into(), is_dir: false },
            TransferSource { path: src_dir.join("b.txt"), name: "b.txt".into(), is_dir: false },
        ],
        destination: TextField::with_cursor_at(dst_dir.to_string_lossy(), 0),
    }));

    handle_confirm_transfer_key(&mut app, key(KeyCode::Enter)).unwrap();

    assert_eq!(fs::read(dst_dir.join("a.txt")).unwrap(), b"aaa");
    assert_eq!(fs::read(dst_dir.join("b.txt")).unwrap(), b"bbb");
    assert!(src_dir.join("a.txt").exists(), "copy should leave the sources alone");
    assert!(src_dir.join("b.txt").exists());
}
