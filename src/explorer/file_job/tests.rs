use std::fs;

use crossterm::event::KeyCode;

use super::*;
use crate::test_support::{key, test_app, unique_scratch_dir};


use super::running_for_test as running;


/// Reported: a large folder froze the app. The job runs on its own, the
/// toast saying how far along it is.
#[test]
fn a_running_copy_shows_how_far_along_it_is() {
    let (job, _sender) = running(JobKind::Copy, "big-folder");
    assert_eq!(job.message(), "Copying big-folder: counting... · Esc stops");

    job.progress.total_bytes.store(4 * 1024 * 1024 * 1024, Ordering::Relaxed);
    job.progress.done_bytes.store(1024 * 1024 * 1024, Ordering::Relaxed);
    job.progress.counted.store(true, Ordering::Relaxed);
    assert_eq!(job.message(), "Copying big-folder: 25% (1.0 G of 4.0 G) · Esc stops");
}


#[test]
fn a_delete_counts_entries_and_the_recycle_bin_its_percentage() {
    let (job, _sender) = running(JobKind::Delete, "old");
    job.progress.total_items.store(400, Ordering::Relaxed);
    job.progress.done_items.store(100, Ordering::Relaxed);
    job.progress.counted.store(true, Ordering::Relaxed);
    assert_eq!(job.message(), "Deleting old: 25% (100 of 400 entries) · Esc stops");

    let (job, _sender) = running(JobKind::Recycle, "old");
    assert_eq!(job.message(), "Deleting old... 0:00 · Esc stops", "the shell says nothing until it has looked the tree over");
    job.progress.total_items.store(1000, Ordering::Relaxed);
    job.progress.done_items.store(420, Ordering::Relaxed);
    job.progress.counted.store(true, Ordering::Relaxed);
    assert_eq!(job.message(), "Deleting old: 42% · Esc stops", "the shell's own units: only the percentage");
}


/// Reported: the progress moved only when a key was pressed. A running
/// job asks for a redraw every `REFRESH_INTERVAL`, not more often.
#[test]
fn a_running_job_asks_for_a_redraw_now_and_then() {
    let mut app = test_app(unique_scratch_dir("file-job"));
    let (mut job, _sender) = running(JobKind::Copy, "x");
    job.next_refresh = Instant::now();
    app.file_job = Some(job);

    assert!(poll_pending_file_job(&mut app), "due: redraw");
    assert!(!poll_pending_file_job(&mut app), "just redrawn");
    assert!(app.file_job.is_some(), "still running");
}


#[test]
fn the_bar_fills_with_what_is_done() {
    let (job, _sender) = running(JobKind::Copy, "x");
    assert_eq!(job.fraction(), None, "counting: only busy");
    job.progress.total_bytes.store(400, Ordering::Relaxed);
    job.progress.done_bytes.store(100, Ordering::Relaxed);
    job.progress.counted.store(true, Ordering::Relaxed);
    assert_eq!(job.fraction(), Some(0.25));

    let (job, _sender) = running(JobKind::Delete, "empty");
    job.progress.counted.store(true, Ordering::Relaxed);
    assert_eq!(job.fraction(), Some(1.0), "nothing to do is all done");
}


#[test]
fn a_finished_job_reloads_and_reports_nothing_when_all_went_well() {
    let mut app = test_app(unique_scratch_dir("file-job"));
    let (job, sender) = running(JobKind::Copy, "x");
    app.file_job = Some(job);

    assert!(!poll_pending_file_job(&mut app), "not done yet");
    sender.send(Vec::new()).unwrap();
    assert!(poll_pending_file_job(&mut app));
    assert!(app.file_job.is_none());
    assert!(app.notice.is_none());
}


#[test]
fn a_second_job_waits_for_the_first() {
    let dir = unique_scratch_dir("file-job");
    let file = dir.join("keep.txt");
    fs::write(&file, "x").unwrap();
    let mut app = test_app(dir);
    let (job, _sender) = running(JobKind::Delete, "big-folder");
    app.file_job = Some(job);

    begin_delete(&mut app, vec![DeleteEntry { path: file.clone(), name: "keep.txt".into(), is_dir: false, size: 1 }], true);

    assert!(file.exists(), "not started");
    assert!(app.notice.as_ref().is_some_and(|notice| notice.text.contains("Still deleting big-folder")), "{:?}", app.notice);
}


/// Requested: `Esc` asks before a copy is stopped; `Y` stops it, and the
/// end says what was done stays.
#[test]
fn esc_asks_and_y_stops_the_job() {
    let mut app = test_app(unique_scratch_dir("file-job"));
    let (job, sender) = running(JobKind::Copy, "big-folder");
    app.file_job = Some(job);

    crate::command_line::handle_browsing_key(&mut app, key(KeyCode::Esc)).unwrap();
    assert!(matches!(app.overlay, Some(Overlay::ConfirmStopJob)));

    handle_confirm_stop_job_key(&mut app, key(KeyCode::Char('y'))).unwrap();
    assert!(app.overlay.is_none());
    assert!(app.file_job.as_ref().unwrap().cancel.load(Ordering::Relaxed), "asked to stop");
    assert_eq!(app.file_job.as_ref().unwrap().message(), "Copying big-folder: stopping...");

    sender.send(Vec::new()).unwrap();
    poll_pending_file_job(&mut app);
    assert!(app.notice.as_ref().is_some_and(|notice| notice.text.contains("stopped")));
}


#[test]
fn esc_answer_lets_the_job_go_on() {
    let mut app = test_app(unique_scratch_dir("file-job"));
    let (job, _sender) = running(JobKind::Move, "big-folder");
    app.file_job = Some(job);
    app.overlay = Some(Overlay::ConfirmStopJob);

    handle_confirm_stop_job_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(app.overlay.is_none());
    assert!(!app.file_job.as_ref().unwrap().cancel.load(Ordering::Relaxed));
}


/// On one drive a move is a rename, tried before anything is counted:
/// counting a tree just to rename it took as long as the walk.
#[test]
fn a_move_on_one_drive_renames_without_counting() {
    let dir = unique_scratch_dir("file-job");
    fs::create_dir_all(dir.join("big").join("sub")).unwrap();
    fs::write(dir.join("big").join("sub").join("a.txt"), "12345").unwrap();
    let progress = Arc::new(Progress::default());
    let entries = [JobEntry { path: dir.join("big"), name: "big".into(), to: Some(dir.join("moved")) }];

    let failures = run(JobKind::Move, &entries, &progress, &Arc::new(AtomicBool::new(false)));

    assert!(failures.is_empty());
    assert!(!dir.join("big").exists());
    assert_eq!(fs::read_to_string(dir.join("moved").join("sub").join("a.txt")).unwrap(), "12345");
    assert_eq!(progress.total_items.load(Ordering::Relaxed), 0, "nothing was counted");
}


/// Reported risk: a folder copied into its own subfolder fills the disk.
#[test]
fn a_copy_or_move_into_itself_is_refused() {
    let dir = unique_scratch_dir("file-job");
    fs::create_dir_all(dir.join("a")).unwrap();
    let mut app = test_app(dir.clone());

    begin_transfer(&mut app, TransferOp::Copy, vec![JobEntry { path: dir.join("a"), name: "a".into(), to: Some(dir.join("a").join("copy")) }]);

    assert!(app.file_job.is_none(), "not started");
    assert!(app.notice.as_ref().is_some_and(|notice| notice.text == "Can't copy a into itself"), "{:?}", app.notice);
    assert!(!dir.join("a").join("copy").exists());
}
