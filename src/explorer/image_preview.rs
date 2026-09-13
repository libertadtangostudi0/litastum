use std::fs;
use std::path::{Path, PathBuf};

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


/// `F3` on an image file (`Mode::ImagePreview`): which image is
/// currently shown, and the decoded, render-ready protocol
/// (`ratatui_image`'s own `StatefulProtocol`) the right panel's area
/// gets drawn with instead of its usual file listing
/// (`ui::draw`/`ui::draw_image_preview`). `Left`/`Right` cycle through
/// every other supported image file in the same directory --
/// requested directly ("просмотр изображений клавишами влево вправо
/// внутри директории"), not just a one-shot single-file preview.
pub struct ImagePreviewState {
    images: Vec<PathBuf>,
    index: usize,
    picker: Picker,
    protocol: StatefulProtocol,
}

impl ImagePreviewState {
    /// Opens a preview session rooted at `dir`, starting on
    /// `initial_path` (the file `F3` was actually pressed on) -- `None`
    /// if `initial_path` isn't a supported image, `dir` can't be read,
    /// or the initial image itself fails to decode (a corrupt or
    /// truncated file, or a misleading extension). `picker` is
    /// `app.image_picker` -- queried once, at startup, against the real
    /// terminal (`Picker::from_query_stdio`, `main.rs`), so this reuses
    /// whichever rendering protocol (Sixel/Kitty/iTerm2, or half-blocks
    /// if the terminal didn't answer) that query actually found, rather
    /// than forcing half-blocks unconditionally -- a first version did
    /// exactly that, reported directly as looking unacceptably bad for
    /// anything but a blocky thumbnail; the query is safe to trust for
    /// quality here specifically because it never blocks startup either
    /// way (real terminals answer immediately, Windows ConPTY's
    /// unreliable delivery just falls back after its own 2-second
    /// timeout, see `Picker::from_query_stdio`'s own doc comment).
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
        let protocol = load_protocol(picker, &images[index])?;

        Some(Self { images, index, picker: picker.clone(), protocol })
    }

    pub fn current_path(&self) -> &Path {
        &self.images[self.index]
    }

    /// The live decoded image, for `ui::draw_image_preview` to render
    /// via `ratatui_image::StatefulImage`.
    pub fn protocol_mut(&mut self) -> &mut StatefulProtocol {
        &mut self.protocol
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

    /// Shared by `next`/`prev` -- a no-op (current image stays showing)
    /// if there's only one image in the directory, or if the new one
    /// fails to decode, rather than blanking the preview on a corrupt
    /// neighbor.
    fn step(&mut self, delta: usize) {
        if self.images.len() <= 1 {
            return;
        }
        let new_index = (self.index + delta) % self.images.len();
        let Some(protocol) = load_protocol(&self.picker, &self.images[new_index]) else {
            return;
        };
        self.index = new_index;
        self.protocol = protocol;
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
/// entry under the cursor, a directory, or an unsupported/undecodable
/// file), same "couldn't act on this" convention as the rest of this
/// codebase. Switches the *right* panel to active
/// (`app.active = 1`) regardless of which panel `F3` was actually
/// pressed from -- requested directly ("выбрав правую активную
/// панель"), since that's the panel whose own area now shows the
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


#[cfg(test)]
mod tests {
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

        #[test]
        fn open_fails_when_the_initial_image_cannot_be_decoded() {
            let dir = unique_scratch_dir("image-preview");
            let path = dir.join("fake.png");
            fs::write(&path, b"not actually a png").unwrap();

            assert!(ImagePreviewState::open(&Picker::halfblocks(), &dir, &path).is_none());
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
}
