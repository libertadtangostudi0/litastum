use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use tracing::warn;

use crate::app::{App, Mode};

/// Extensions `F3`'s image preview understands -- just the three
/// formats actually requested (`TODO/viewer.md`). More (`.gif`,
/// `.webp`, ...) can follow later without much extra work -- the
/// underlying `image` crate already supports decoding them -- but
/// widening this list also means widening `Cargo.toml`'s own `image`
/// feature flags, kept deliberately narrow for now.
const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp"];

/// Whether `path` is a file `F3`'s image preview knows how to open --
/// checked by extension only (a mismatched extension just fails to
/// decode later, same as any other "corrupt/unreadable" case).
pub fn is_supported_image(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| SUPPORTED_EXTENSIONS.iter().any(|supported| ext.eq_ignore_ascii_case(supported)))
}


/// What `ui::draw_image_preview` should actually render this frame --
/// `ImagePreviewState::frame_mut`'s own return type. A thin, borrowing
/// view over `ImagePreviewState`'s private `Display` rather than
/// exposing that enum itself, so this module stays the only place that
/// can construct a `Display::Ready` (i.e. only a real, successfully
/// decoded protocol ever reaches the renderer).
pub enum PreviewFrame<'a> {
    Ready(&'a mut StatefulProtocol),
    Loading,
    Failed,
}

/// `image::decode()` + `Picker::new_resize_protocol`'s own resize/
/// re-encode step (`ui::draw_image_preview`'s own doc comment on why
/// `FilterType::Lanczos3`, the slow-but-good filter, is worth it here)
/// together cost real, user-visible time for anything but a tiny
/// image -- reported directly, both for the very first `F3` open and
/// for every `Left`/`Right` switch afterward. Both used to run
/// synchronously, inline in the key handler, blocking the entire UI
/// (single-threaded event loop) for however long that took. Decoded on
/// a background thread now (`spawn_decode`) instead: `open`/`step`
/// return immediately, `poll` (called every main-loop tick while
/// `pending` is `Some` -- see `main.rs::wait_for_event`) picks up the
/// finished result once it's actually ready.
enum Display {
    Ready(StatefulProtocol),
    Loading,
    Failed,
}

/// A decode in flight -- `pending`'s own `Some` case. Holds nothing but
/// the receiving half of the one-shot channel `spawn_decode` hands
/// back; there's no request id/generation counter needed to detect a
/// *stale* result superseded by a further, faster `Left`/`Right` press
/// in the meantime, because there's only ever one `PendingDecode`
/// alive in `self` at a time -- starting a new one simply replaces
/// (drops) this `Receiver`, and the old background thread's own
/// eventual `sender.send(..)` against a receiver nobody's listening on
/// anymore just fails silently (the thread still finishes and exits
/// normally, its result just goes nowhere).
struct PendingDecode {
    receiver: Receiver<Option<StatefulProtocol>>,
}

fn spawn_decode(picker: &Picker, path: PathBuf) -> PendingDecode {
    let (sender, receiver) = std::sync::mpsc::channel();
    let picker = picker.clone();
    thread::spawn(move || {
        let result = load_protocol(&picker, &path);
        let _ = sender.send(result);
    });
    PendingDecode { receiver }
}


/// `F3` on an image file (`Mode::ImagePreview`): which image is
/// currently shown, and the decoded, render-ready protocol
/// (`ratatui_image`'s own `StatefulProtocol`) the right panel's area
/// gets drawn with instead of its usual file listing
/// (`ui::draw`/`ui::draw_image_preview`). `Left`/`Right` cycle through
/// every other supported image file in the same directory -- requested
/// directly (browsing images with the Left/Right keys within a
/// directory), not just a one-shot single-file preview.
pub struct ImagePreviewState {
    images: Vec<PathBuf>,
    index: usize,
    picker: Picker,
    display: Display,
    pending: Option<PendingDecode>,
}

impl ImagePreviewState {
    /// Opens a preview session rooted at `dir`, starting on
    /// `initial_path` (the file `F3` was actually pressed on) -- `None`
    /// only if `initial_path` isn't a supported image, or `dir` can't
    /// be read; both are cheap, synchronous checks, so `open` itself
    /// never blocks. The initial image's own decode is *not* one of
    /// those checks anymore -- it starts in `Display::Loading` and
    /// resolves asynchronously (`poll`), even for a corrupt/truncated
    /// first file (`Display::Failed` once that's confirmed, rather than
    /// `open` itself returning `None` and never entering the preview
    /// at all the way a synchronous decode failure used to). `picker`
    /// is `app.image_picker` -- queried once, at startup, against the
    /// real terminal (`Picker::from_query_stdio`, `main.rs`), so this
    /// reuses whichever rendering protocol (Sixel/Kitty/iTerm2, or
    /// half-blocks if the terminal didn't answer) that query actually
    /// found, rather than forcing half-blocks unconditionally -- a
    /// first version did exactly that, reported directly as looking
    /// unacceptably bad for anything but a blocky thumbnail; the query
    /// is safe to trust for quality here specifically because it never
    /// blocks startup either way (real terminals answer immediately,
    /// Windows ConPTY's unreliable delivery just falls back after its
    /// own 2-second timeout, see `Picker::from_query_stdio`'s own doc
    /// comment).
    pub fn open(picker: &Picker, dir: &Path, initial_path: &Path) -> Option<Self> {
        if !is_supported_image(initial_path) {
            return None;
        }

        let mut images: Vec<PathBuf> = fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| is_supported_image(path))
            .collect();
        // Case-insensitive filename sort -- close to `Panel`'s own
        // listing order, but not identical: `Panel::compare_entries`'s
        // natural (digit-run-aware) sort is `pub(super)` to the `panel`
        // module and not worth exposing further just for this. A minor
        // "10.jpg before 2.jpg" wrinkle in a long numbered sequence is
        // an accepted, narrow gap here.
        images.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));

        let index = images.iter().position(|path| path == initial_path)?;
        let pending = spawn_decode(picker, images[index].clone());

        Some(Self { images, index, picker: picker.clone(), display: Display::Loading, pending: Some(pending) })
    }

    pub fn current_path(&self) -> &Path {
        &self.images[self.index]
    }

    /// What `ui::draw_image_preview` should render this frame -- see
    /// `PreviewFrame`'s own doc comment.
    pub fn frame_mut(&mut self) -> PreviewFrame<'_> {
        match &mut self.display {
            Display::Ready(protocol) => PreviewFrame::Ready(protocol),
            Display::Loading => PreviewFrame::Loading,
            Display::Failed => PreviewFrame::Failed,
        }
    }

    /// Whether a background decode is currently in flight --
    /// `main.rs::wait_for_event` polls more often than its own default
    /// idle cadence while this is `true`, purely so a finished decode
    /// gets drawn within one short tick instead of sitting there
    /// already-ready but unseen until the next real keypress/mouse
    /// event happens to wake the main loop up anyway.
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Checks whether the current background decode has finished,
    /// applying the result in place if so. `false` (a no-op) with
    /// nothing pending, or if the decode genuinely hasn't finished yet.
    /// `true` means something changed and the caller should redraw --
    /// called from `main.rs::wait_for_event`'s own poll loop, and once
    /// more from `ui::draw` itself right before rendering, so a result
    /// that arrived in between two loop ticks (or during the brief
    /// window a real keyboard/mouse event was also being handled) is
    /// never left stale for a whole extra frame.
    pub fn poll(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        match pending.receiver.try_recv() {
            Err(TryRecvError::Empty) => false,
            Ok(Some(protocol)) => {
                self.display = Display::Ready(protocol);
                self.pending = None;
                true
            }
            // A clean decode failure and a disconnected channel (the
            // background thread panicked mid-decode) both mean "this
            // is never going to finish successfully" -- treated the
            // same way: give up waiting rather than leaving `pending`
            // `Some` forever, which would keep `wait_for_event` polling
            // at the tighter interval for nothing. Keeps showing
            // whatever was already on screen (`Display::Ready(_)`, a
            // still-valid previous image) rather than blanking a
            // working preview over one corrupt neighbor -- only
            // actually shows `Failed` when there was nothing to fall
            // back to yet (the very first image in a fresh session).
            Ok(None) | Err(TryRecvError::Disconnected) => {
                if !matches!(self.display, Display::Ready(_)) {
                    self.display = Display::Failed;
                }
                self.pending = None;
                true
            }
        }
    }

    /// `Right`: advances to the next image file in the directory,
    /// wrapping back to the first past the last.
    pub fn next(&mut self) {
        self.step(1);
    }

    /// `Left`: mirror of `next`, wrapping back to the last past the
    /// first.
    pub fn prev(&mut self) {
        self.step(self.images.len() - 1); // (-1) mod len, without signed arithmetic
    }

    /// Shared by `next`/`prev` -- a no-op if there's only one image in
    /// the directory. `index` (and so `current_path()`/the title bar
    /// `ui::draw_image_preview` shows) moves *immediately*, synchronously
    /// -- only the actual pixels lag behind, via a fresh background
    /// decode (`spawn_decode`), so `Left`/`Right` itself never blocks
    /// no matter how slow decoding the new image turns out to be.
    /// `display` is deliberately left untouched here, still showing
    /// whatever the *previous* image decoded to -- switching to
    /// `Display::Loading` on every press was tried and reverted: it
    /// meant a visible blank/placeholder flash on every single
    /// `Left`/`Right`, even for images that decode fast enough nobody
    /// would otherwise notice the switch wasn't instant. Keeping the
    /// old pixels up until `poll` actually has a replacement reads as
    /// "already moved on, still catching up visually" instead.
    fn step(&mut self, delta: usize) {
        if self.images.len() <= 1 {
            return;
        }
        let new_index = (self.index + delta) % self.images.len();
        self.index = new_index;
        self.pending = Some(spawn_decode(&self.picker, self.images[new_index].clone()));
    }
}

fn load_protocol(picker: &Picker, path: &Path) -> Option<StatefulProtocol> {
    let reader = match image::ImageReader::open(path) {
        Ok(reader) => reader,
        Err(err) => {
            warn!(path = %path.display(), %err, "failed to open image for preview");
            return None;
        }
    };
    match reader.decode() {
        Ok(image) => Some(picker.new_resize_protocol(image)),
        Err(err) => {
            warn!(path = %path.display(), %err, "failed to decode image for preview");
            None
        }
    }
}


/// `F3`: opens `Mode::ImagePreview` for the active panel's own selected
/// entry, if it's a supported image -- a silent no-op otherwise (no
/// entry under the cursor, a directory, or an unsupported file/unreadable
/// directory), same "couldn't act on this" convention as the rest of
/// this codebase. Switches the *right* panel to active
/// (`app.active = 1`) regardless of which panel `F3` was actually
/// pressed from -- requested directly (the right panel should become
/// the active one), since that's the panel whose own area now shows the
/// preview instead of a file listing, and `Left`/`Right` cycling the
/// preview only makes sense once that panel is the one actually
/// focused.
pub fn open_preview(app: &mut App) {
    let panel = app.active_panel();
    let Some(path) = panel.selected_path() else {
        return;
    };
    let dir = panel.path.clone();

    let Some(state) = ImagePreviewState::open(&app.image_picker, &dir, &path) else {
        return;
    };

    app.active = 1;
    app.mode = Mode::ImagePreview(state);
}


/// Key handling while `Mode::ImagePreview` is showing: `Left`/`Right`
/// cycle to the previous/next image in the directory; `Esc` or `F3`
/// again closes the preview back to `Mode::Browsing`. Nothing else is
/// bound -- this is a read-only preview, not a second file browser.
pub fn handle_image_preview_key(app: &mut App, key: KeyEvent) {
    let Mode::ImagePreview(state) = &mut app.mode else {
        return;
    };

    match key.code {
        KeyCode::Left => state.prev(),
        KeyCode::Right => state.next(),
        KeyCode::Esc | KeyCode::F(3) => app.mode = Mode::Browsing,
        _ => {}
    }
}


/// Whether `Mode::ImagePreview`'s current decode is still in flight --
/// `main.rs::wait_for_event` polls more often than its own default
/// idle cadence while this is `true`, so a background decode finishing
/// while otherwise idle gets drawn within one short tick rather than
/// waiting for the next real input event to happen to wake the main
/// loop up. `false` outside `Mode::ImagePreview` entirely.
pub fn is_image_decode_pending(app: &App) -> bool {
    matches!(&app.mode, Mode::ImagePreview(state) if state.is_loading())
}

/// Checks whether `Mode::ImagePreview`'s pending decode (if any) has
/// finished, applying it in place -- see `ImagePreviewState::poll`.
/// `false` (a no-op) outside `Mode::ImagePreview` or with nothing
/// pending.
pub fn poll_pending_image_decode(app: &mut App) -> bool {
    let Mode::ImagePreview(state) = &mut app.mode else {
        return false;
    };
    state.poll()
}


#[cfg(test)]
mod tests {
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

            handle_image_preview_key(&mut app, key(KeyCode::Esc));

            assert!(matches!(app.mode, Mode::Browsing));
        }

        #[test]
        fn f3_again_also_closes_the_preview() {
            let mut app = app_in_preview();

            handle_image_preview_key(&mut app, key(KeyCode::F(3)));

            assert!(matches!(app.mode, Mode::Browsing));
        }

        #[test]
        fn is_a_noop_outside_image_preview_mode() {
            let mut app = app_in_preview();
            app.mode = Mode::Browsing;

            handle_image_preview_key(&mut app, key(KeyCode::Esc));

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
}
