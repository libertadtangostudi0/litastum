use std::time::Duration;

use super::*;
use crate::test_support::unique_scratch_dir;

/// A real, valid 1x1 PNG -- `ImagePreviewState::open` has to
/// actually decode a fixture, so plausible-looking-but-fake bytes
/// would fail exactly like a corrupt file.
fn write_test_png(path: &Path) {
    let png_1x1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77,
        0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    fs::write(path, png_1x1).unwrap();
}

/// Drives `poll()` until the current decode resolves one way or
/// another (`is_loading()` goes `false`), or gives up after a
/// generous timeout -- the fixture images here are tiny, so a real
/// background decode should resolve in well under this on any
/// machine; a test that never resolves indicates a real bug
/// (`poll` failing to notice a finished/disconnected channel)
/// rather than something to paper over with an even longer wait.
fn wait_until_not_loading(state: &mut ImagePreviewState) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while state.is_loading() {
        assert!(std::time::Instant::now() < deadline, "background decode never finished");
        state.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn is_supported_image_recognizes_jpg_png_bmp_case_insensitively() {
    assert!(is_supported_image(Path::new("photo.jpg")));
    assert!(is_supported_image(Path::new("PHOTO.JPG")));
    assert!(is_supported_image(Path::new("photo.jpeg")));
    assert!(is_supported_image(Path::new("photo.png")));
    assert!(is_supported_image(Path::new("photo.bmp")));
}

#[test]
fn is_supported_image_rejects_other_extensions() {
    assert!(!is_supported_image(Path::new("notes.txt")));
    assert!(!is_supported_image(Path::new("archive.gif")));
    assert!(!is_supported_image(Path::new("no_extension")));
}

mod image_preview_state_tests {
    use super::*;

    #[test]
    fn open_fails_for_a_non_image_path() {
        let dir = unique_scratch_dir("image-preview");
        let path = dir.join("notes.txt");
        fs::write(&path, "hi").unwrap();

        assert!(ImagePreviewState::open(&Picker::halfblocks(), &dir, &path).is_none());
    }

    /// Regression coverage for the actual point of the async
    /// rewrite: `open` itself must never block on the decode, even
    /// though the very first image can still legitimately fail to
    /// decode.
    #[test]
    fn open_starts_loading_immediately_without_blocking() {
        let dir = unique_scratch_dir("image-preview");
        write_test_png(&dir.join("a.png"));

        let state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &dir.join("a.png")).unwrap();

        assert!(state.is_loading(), "open() itself should never wait for the decode to finish");
    }

    #[test]
    fn a_successful_decode_eventually_becomes_ready() {
        let dir = unique_scratch_dir("image-preview");
        write_test_png(&dir.join("a.png"));
        let mut state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &dir.join("a.png")).unwrap();

        wait_until_not_loading(&mut state);

        assert!(matches!(state.frame_mut(), PreviewFrame::Ready(_)));
    }

    /// Reworked from the old synchronous `open_fails_when_the_initial_
    /// image_cannot_be_decoded`: a corrupt *first* image no longer
    /// makes `open` itself return `None` (there's nothing yet to
    /// fall back to the way a corrupt *neighbor* can fall back to
    /// the still-showing previous image) -- it resolves to
    /// `PreviewFrame::Failed` once the background decode reports
    /// back, instead.
    #[test]
    fn a_failed_initial_decode_resolves_to_failed() {
        let dir = unique_scratch_dir("image-preview");
        let path = dir.join("fake.png");
        fs::write(&path, b"not actually a png").unwrap();
        let mut state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &path).unwrap();

        wait_until_not_loading(&mut state);

        assert!(matches!(state.frame_mut(), PreviewFrame::Failed));
    }

    #[test]
    fn open_succeeds_and_lists_every_supported_image_in_the_directory() {
        let dir = unique_scratch_dir("image-preview");
        write_test_png(&dir.join("a.png"));
        write_test_png(&dir.join("b.png"));
        fs::write(dir.join("notes.txt"), "not an image").unwrap();

        let state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &dir.join("a.png")).unwrap();

        assert_eq!(state.current_path(), dir.join("a.png"));
    }

    #[test]
    fn next_and_prev_wrap_around_the_directorys_image_list() {
        let dir = unique_scratch_dir("image-preview");
        write_test_png(&dir.join("a.png"));
        write_test_png(&dir.join("b.png"));
        let mut state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &dir.join("a.png")).unwrap();

        state.next();
        assert_eq!(state.current_path(), dir.join("b.png"));
        state.next();
        assert_eq!(state.current_path(), dir.join("a.png"), "should wrap back to the first");

        state.prev();
        assert_eq!(state.current_path(), dir.join("b.png"), "should wrap back to the last");
    }

    #[test]
    fn next_is_a_noop_with_only_one_image_in_the_directory() {
        let dir = unique_scratch_dir("image-preview");
        write_test_png(&dir.join("only.png"));
        let mut state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &dir.join("only.png")).unwrap();

        state.next();

        assert_eq!(state.current_path(), dir.join("only.png"));
    }

    /// The actual point of the async rewrite, end to end: moving to
    /// the next image is instant (the index/title move immediately,
    /// synchronously) even before its own decode has finished, and
    /// the previous image's own pixels keep showing in the
    /// meantime rather than flashing to a blank/loading state on
    /// every single switch.
    #[test]
    fn next_moves_the_index_immediately_and_keeps_the_previous_pixels_until_the_new_ones_are_ready() {
        let dir = unique_scratch_dir("image-preview");
        write_test_png(&dir.join("a.png"));
        write_test_png(&dir.join("b.png"));
        let mut state = ImagePreviewState::open(&Picker::halfblocks(), &dir, &dir.join("a.png")).unwrap();
        wait_until_not_loading(&mut state);
        assert!(matches!(state.frame_mut(), PreviewFrame::Ready(_)), "sanity: the first image finished decoding");

        state.next();

        assert_eq!(state.current_path(), dir.join("b.png"), "the index/title should move immediately, not wait for the new decode");
        assert!(state.is_loading(), "the new image's own decode should now be in flight");
        assert!(matches!(state.frame_mut(), PreviewFrame::Ready(_)), "should still show the previous image's pixels while the new one decodes");

        wait_until_not_loading(&mut state);
        assert!(matches!(state.frame_mut(), PreviewFrame::Ready(_)), "and the new pixels should be showing once the decode actually finishes");
    }
}

mod open_preview_tests {
    use super::*;
    use crate::test_support::test_app;

    #[test]
    fn opens_the_preview_and_switches_the_right_panel_active() {
        let dir = unique_scratch_dir("image-preview-open");
        write_test_png(&dir.join("photo.png"));
        let mut app = test_app(dir.clone());
        app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "photo.png").unwrap();

        open_preview(&mut app);

        assert!(matches!(app.mode, Mode::ImagePreview(_)));
        assert_eq!(app.active, 1, "the right panel should become active");
    }

    #[test]
    fn is_a_noop_on_a_non_image_file() {
        let dir = unique_scratch_dir("image-preview-open");
        fs::write(dir.join("notes.txt"), "hi").unwrap();
        let mut app = test_app(dir);
        app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "notes.txt").unwrap();

        open_preview(&mut app);

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn is_a_noop_on_an_empty_panel() {
        let dir = unique_scratch_dir("image-preview-open");
        let mut app = test_app(dir);
        app.panels[0].entries.clear();

        open_preview(&mut app);

        assert!(matches!(app.mode, Mode::Browsing));
    }
}

mod handle_image_preview_key_tests {
    use super::*;
    use crate::test_support::{key, test_app};

    fn app_in_preview() -> App {
        let dir = unique_scratch_dir("image-preview-keys");
        write_test_png(&dir.join("a.png"));
        let mut app = test_app(dir.clone());
        app.mode = Mode::ImagePreview(ImagePreviewState::open(&app.image_picker, &dir, &dir.join("a.png")).unwrap());
        app
    }

    #[test]
    fn esc_closes_the_preview() {
        let mut app = app_in_preview();

        handle_image_preview_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn f3_again_also_closes_the_preview() {
        let mut app = app_in_preview();

        handle_image_preview_key(&mut app, key(KeyCode::F(3))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn is_a_noop_outside_image_preview_mode() {
        let mut app = app_in_preview();
        app.mode = Mode::Browsing;

        handle_image_preview_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}

mod polling_helper_tests {
    use super::*;
    use crate::test_support::test_app;

    #[test]
    fn is_image_decode_pending_is_false_outside_image_preview_mode() {
        let app = test_app(unique_scratch_dir("image-preview-poll"));
        assert!(!is_image_decode_pending(&app));
    }

    #[test]
    fn poll_pending_image_decode_is_a_noop_outside_image_preview_mode() {
        let mut app = test_app(unique_scratch_dir("image-preview-poll"));
        assert!(!poll_pending_image_decode(&mut app));
    }

    #[test]
    fn is_image_decode_pending_tracks_the_real_state_end_to_end() {
        let dir = unique_scratch_dir("image-preview-poll");
        write_test_png(&dir.join("a.png"));
        let mut app = test_app(dir.clone());
        app.mode = Mode::ImagePreview(ImagePreviewState::open(&app.image_picker, &dir, &dir.join("a.png")).unwrap());

        assert!(is_image_decode_pending(&app), "should start pending");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while is_image_decode_pending(&app) {
            assert!(std::time::Instant::now() < deadline, "background decode never finished");
            poll_pending_image_decode(&mut app);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
