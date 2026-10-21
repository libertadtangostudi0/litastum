use std::fs;

use crossterm::event::KeyCode;

use super::*;
use crate::test_support::{key, test_app, unique_scratch_dir};


/// A job that never finishes, as a large folder seems to.
fn running(kind: JobKind, label: &str) -> (FileJob, mpsc::Sender<Vec<(String, io::Error)>>) {
    let (sender, done) = mpsc::channel();
    let job = FileJob { kind, label: label.to_string(), total: 1, progress: Arc::new(Progress::default()), cancel: Arc::new(AtomicBool::new(false)), started: Instant::now(), done };
    (job, sender)
}


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
fn a_delete_for_good_counts_entries_and_the_recycle_bin_its_time() {
    let (job, _sender) = running(JobKind::Delete, "old");
    job.progress.total_items.store(400, Ordering::Relaxed);
    job.progress.done_items.store(100, Ordering::Relaxed);
    job.progress.counted.store(true, Ordering::Relaxed);
    assert_eq!(job.message(), "Deleting old: 25% (100 of 400 entries) · Esc stops");

    let (job, _sender) = running(JobKind::Recycle, "old");
    assert_eq!(job.message(), "Deleting old... 0:00", "no progress of its own, and no stopping, yet");
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


/// On one drive a move is a rename: done at once, its bytes counted as
/// done. (Across drives it's `copy_tree` then `delete_tree`, tested on
/// their own.)
#[test]
fn a_move_renames_and_counts_the_bytes_as_done() {
    let dir = unique_scratch_dir("file-job");
    fs::write(dir.join("a.txt"), "12345").unwrap();
    let progress = Progress::default();
    work::count(&dir.join("a.txt"), &progress, &AtomicBool::new(false));

    move_entry(&dir.join("a.txt"), &dir.join("b.txt"), &progress, &AtomicBool::new(false)).unwrap();

    assert!(!dir.join("a.txt").exists());
    assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "12345");
    assert_eq!(progress.done_bytes.load(Ordering::Relaxed), 5);
}
