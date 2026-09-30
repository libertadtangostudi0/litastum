use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use tracing::warn;

use crate::app::{App, Mode};
use crate::command_line::Effect;

/// Extensions the image preview opens -- the formats requested. More
/// also need `Cargo.toml`'s `image` features widened, kept narrow.
const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp"];

/// By extension only; a mismatched one just fails to decode later.
pub fn is_supported_image(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| SUPPORTED_EXTENSIONS.iter().any(|supported| ext.eq_ignore_ascii_case(supported)))
}


/// What `ui::draw_image_preview` renders this frame: a borrowing view
/// over the private `Display`, so only this module can produce `Ready`
/// (a successfully decoded protocol).
pub enum PreviewFrame<'a> {
    Ready(&'a mut StatefulProtocol),
    Loading,
    Failed,
}

/// What is on screen. Decoding runs on a background thread
/// (`spawn_decode`) because decode plus resize took visible time and
/// froze the UI; `poll` applies the result. History: docs/history/image-preview.md.
enum Display {
    Ready(StatefulProtocol),
    Loading,
    Failed,
}

/// A decode in flight. Only one exists at a time: starting another drops
/// this `Receiver`, and the old thread's `send` fails silently -- no
/// generation counter needed for stale results.
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


/// `F3` on an image (`Mode::ImagePreview`): the directory's images, the
/// current one, and its decoded protocol, drawn in place of the right
/// panel's listing. `Left`/`Right` cycle through the directory.
pub struct ImagePreviewState {
    images: Vec<PathBuf>,
    index: usize,
    picker: Picker,
    display: Display,
    pending: Option<PendingDecode>,
}

impl ImagePreviewState {
    /// Opens a preview on `initial_path` among `dir`'s images. `None` if
    /// it isn't a supported image or `dir` can't be read -- both cheap;
    /// the decode itself starts in `Loading` and resolves in `poll`.
    /// `picker` is `app.image_picker`, queried from the real terminal at
    /// startup. History: docs/history/image-preview.md.
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
        // Case-insensitive by name, not `Panel`'s natural sort (private to
        // `panel`); "10.jpg before 2.jpg" is an accepted gap.
        images.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));

        let index = images.iter().position(|path| path == initial_path)?;
        let pending = spawn_decode(picker, images[index].clone());

        Some(Self { images, index, picker: picker.clone(), display: Display::Loading, pending: Some(pending) })
    }

    pub fn current_path(&self) -> &Path {
        &self.images[self.index]
    }

    /// See `PreviewFrame`.
    pub fn frame_mut(&mut self) -> PreviewFrame<'_> {
        match &mut self.display {
            Display::Ready(protocol) => PreviewFrame::Ready(protocol),
            Display::Loading => PreviewFrame::Loading,
            Display::Failed => PreviewFrame::Failed,
        }
    }

    /// Whether a decode is in flight -- `wait_for_event` polls faster
    /// meanwhile, so the result shows without waiting for input.
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Applies a finished decode; `true` if the screen should redraw.
    /// Called from `wait_for_event`'s poll and again from `ui::draw`, so a
    /// result arriving between ticks isn't left a frame stale.
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
            // A failed decode or a panicked thread: stop waiting. A
            // previous image stays on screen; `Failed` only when there's
            // nothing to fall back to.
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

    /// Moves `index` (and the title) at once and starts a background
    /// decode. `display` keeps the previous image until `poll` replaces
    /// it: switching to `Loading` on every press flashed a placeholder
    /// even for fast images, and was reverted. History: docs/history/image-preview.md.
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


/// `F3`: opens `Mode::ImagePreview` on the selected entry if it's a
/// supported image, else a no-op. Makes the right panel active -- it's
/// the one showing the preview, where `Left`/`Right` apply.
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


/// `Left`/`Right` cycle images; `Esc` or `F3` close the preview.
pub fn handle_image_preview_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Mode::ImagePreview(state) = &mut app.mode else {
        return Ok(Effect::None);
    };

    match key.code {
        KeyCode::Left => state.prev(),
        KeyCode::Right => state.next(),
        KeyCode::Esc | KeyCode::F(3) => app.mode = Mode::Browsing,
        _ => {}
    }
    Ok(Effect::None)
}


/// Whether an image decode is in flight (see `ImagePreviewState::is_loading`);
/// `false` outside `Mode::ImagePreview`.
pub fn is_image_decode_pending(app: &App) -> bool {
    matches!(&app.mode, Mode::ImagePreview(state) if state.is_loading())
}

/// `ImagePreviewState::poll` for the current screen; `false` outside
/// `Mode::ImagePreview`.
pub fn poll_pending_image_decode(app: &mut App) -> bool {
    let Mode::ImagePreview(state) = &mut app.mode else {
        return false;
    };
    state.poll()
}


#[cfg(test)]
mod tests;
