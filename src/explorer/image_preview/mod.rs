use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{FilterType, Resize};
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
/// over the private entries, so only this module can produce `Ready` (an
/// image encoded for the terminal).
pub enum PreviewFrame<'a> {
    Ready(&'a Protocol),
    Loading,
    Failed,
}

/// How long a job waits for the preview's size (`set_area`, from the
/// first draw) before encoding at `DEFAULT_SIZE`.
const AREA_WAIT: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_millis(200) };
const DEFAULT_SIZE: Size = Size { width: 80, height: 24 };

/// The preview's size in cells, shared with the jobs: the image is
/// resized and encoded for it off the UI thread.
type SharedArea = Arc<Mutex<Option<Size>>>;

/// An image encoded for the terminal at `size`, and the decoded image
/// it came from, to encode again at another size without decoding.
struct Encoded {
    image: Arc<DynamicImage>,
    size: Size,
    protocol: Protocol,
}

/// One image of the preview: being prepared, ready, or failed.
enum Entry {
    /// A job in flight, and its cancel flag. Dropped -- an image switched
    /// past -- it's cancelled: the job stops before its next step
    /// (holding `Right` over large photos started dozens of jobs that all
    /// ran to the end), and its `send` fails silently.
    Pending(Receiver<Option<Encoded>>, Arc<AtomicBool>),
    Ready(Encoded),
    Failed,
}

impl Drop for Entry {
    fn drop(&mut self) {
        if let Entry::Pending(_, cancelled) = self {
            cancelled.store(true, Ordering::Relaxed);
        }
    }
}

/// Where a job starts: a file to decode, or a decoded image to encode at
/// a new size.
enum Source {
    File(PathBuf),
    Decoded(Arc<DynamicImage>),
}

/// Decodes, resizes and encodes an image on a thread of its own -- all
/// of it, since resizing and encoding (Sixel above all) cost far more
/// than decoding and froze the UI when done while drawing (reported:
/// switching images was very slow in Windows Terminal).
fn spawn_job(picker: &Picker, source: Source, area: &SharedArea) -> Entry {
    let (sender, receiver) = std::sync::mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let (picker, area, job_cancelled) = (picker.clone(), area.clone(), cancelled.clone());
    thread::spawn(move || {
        let _ = sender.send(prepare(&picker, source, &area, &job_cancelled));
    });
    Entry::Pending(receiver, cancelled)
}

fn prepare(picker: &Picker, source: Source, area: &SharedArea, cancelled: &AtomicBool) -> Option<Encoded> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let image = match source {
        Source::File(path) => Arc::new(decode(&path)?),
        Source::Decoded(image) => image,
    };
    let size = wait_for_area(area);
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let protocol = picker.new_protocol((*image).clone(), size, Resize::Fit(Some(FilterType::Lanczos3)));
    match protocol {
        Ok(protocol) => Some(Encoded { image, size, protocol }),
        Err(err) => {
            warn!(%err, "failed to encode image for preview");
            None
        }
    }
}

/// The preview's size, once the first draw told it.
fn wait_for_area(area: &SharedArea) -> Size {
    let deadline = Instant::now() + AREA_WAIT;
    loop {
        if let Some(size) = *area.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) {
            return size;
        }
        if Instant::now() >= deadline {
            return DEFAULT_SIZE;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn decode(path: &Path) -> Option<DynamicImage> {
    let reader = match image::ImageReader::open(path) {
        Ok(reader) => reader,
        Err(err) => {
            warn!(path = %path.display(), %err, "failed to open image for preview");
            return None;
        }
    };
    match reader.decode() {
        Ok(image) => Some(image),
        Err(err) => {
            warn!(path = %path.display(), %err, "failed to decode image for preview");
            None
        }
    }
}


/// `F3` on an image (`Mode::ImagePreview`): the directory's images and the
/// current one, drawn in place of the right panel's listing. `Left`/
/// `Right` cycle through the directory. The current image and its two
/// neighbors are kept encoded, the neighbors prepared once the current
/// one is ready, so a switch shows at once. History: docs/history/image-preview.md.
pub struct ImagePreviewState {
    images: Vec<PathBuf>,
    index: usize,
    picker: Picker,
    area: SharedArea,
    entries: HashMap<usize, Entry>,
    /// What's on screen until the current image is ready: switching
    /// showed a placeholder even for fast images, and was reverted.
    shown: Option<Protocol>,
}

impl ImagePreviewState {
    /// Opens a preview on `initial_path` among `dir`'s images, by name (the
    /// tests' way; litastum opens it with the panel's own order,
    /// `open_among`). The image starts `Loading` and resolves in `poll`.
    #[cfg(test)]
    pub fn open(picker: &Picker, dir: &Path, initial_path: &Path) -> Option<Self> {
        let mut images: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.filter_map(|entry| entry.ok()).map(|entry| entry.path()).collect();
        images.sort();
        Self::open_among(picker, images, initial_path)
    }

    /// Opens a preview on `initial_path` among `paths`' images, in that
    /// order -- the panel's (`open_preview`): the preview used to sort by
    /// name on its own, `10.png` right after `1.png` while the panel went
    /// on to `2.png`. `None` if it isn't a supported image among them.
    pub fn open_among(picker: &Picker, paths: Vec<PathBuf>, initial_path: &Path) -> Option<Self> {
        if !is_supported_image(initial_path) {
            return None;
        }
        let images: Vec<PathBuf> = paths.into_iter().filter(|path| is_supported_image(path)).collect();
        let index = images.iter().position(|path| path == initial_path)?;
        let mut state = Self { images, index, picker: picker.clone(), area: SharedArea::default(), entries: HashMap::new(), shown: None };
        state.prepare(index);
        Some(state)
    }

    pub fn current_path(&self) -> &Path {
        &self.images[self.index]
    }

    /// The preview's size in cells, from each draw: images are encoded for
    /// it, again (from their decoded pixels) when it changes.
    pub fn set_area(&mut self, size: Size) {
        let mut area = self.area.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if *area == Some(size) {
            return;
        }
        *area = Some(size);
        drop(area);
        for index in self.entries.keys().copied().collect::<Vec<_>>() {
            if let Some(Entry::Ready(encoded)) = self.entries.get(&index) {
                let source = Source::Decoded(encoded.image.clone());
                self.entries.insert(index, spawn_job(&self.picker, source, &self.area));
            }
        }
    }

    /// See `PreviewFrame`.
    pub fn frame_mut(&mut self) -> PreviewFrame<'_> {
        // A failed image says so, rather than the previous picture showing
        // under its name.
        match (self.entries.get(&self.index), &self.shown) {
            (Some(Entry::Ready(encoded)), _) => PreviewFrame::Ready(&encoded.protocol),
            (Some(Entry::Failed), _) => PreviewFrame::Failed,
            (_, Some(shown)) => PreviewFrame::Ready(shown),
            _ => PreviewFrame::Loading,
        }
    }

    /// Whether the current image is still being prepared --
    /// `wait_for_event` polls faster meanwhile, so it shows without
    /// waiting for input.
    pub fn is_loading(&self) -> bool {
        !matches!(self.entries.get(&self.index), Some(Entry::Ready(_) | Entry::Failed))
    }

    /// Applies finished jobs; `true` if the screen should redraw. Called
    /// from `wait_for_event`'s poll and again from `ui::draw`, so a result
    /// arriving between ticks isn't left a frame stale. Once the current
    /// image is ready its neighbors are prepared.
    pub fn poll(&mut self) -> bool {
        let area = *self.area.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut current_done = false;
        for (&index, entry) in &mut self.entries {
            let Entry::Pending(receiver, _) = entry else {
                continue;
            };
            let outcome = match receiver.try_recv() {
                Err(TryRecvError::Empty) => continue,
                Ok(Some(encoded)) if area.is_some_and(|area| area != encoded.size) => {
                    // Encoded before the size was known, or before it changed.
                    *entry = spawn_job(&self.picker, Source::Decoded(encoded.image), &self.area);
                    continue;
                }
                Ok(Some(encoded)) => Entry::Ready(encoded),
                // A failed decode or a panicked thread: stop waiting.
                Ok(None) | Err(TryRecvError::Disconnected) => Entry::Failed,
            };
            *entry = outcome;
            current_done |= index == self.index;
        }
        if !current_done {
            return false;
        }
        if let Some(Entry::Ready(encoded)) = self.entries.get(&self.index) {
            self.shown = Some(encoded.protocol.clone());
            for neighbor in self.neighbors() {
                self.prepare(neighbor);
            }
        }
        true
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

    /// Moves `index` (and the title) at once; a neighbor prepared ahead
    /// shows right away, else the previous image stays until `poll`.
    /// Images no longer next to the current one are dropped.
    fn step(&mut self, delta: usize) {
        if self.images.len() <= 1 {
            return;
        }
        self.index = (self.index + delta) % self.images.len();
        let keep: Vec<usize> = std::iter::once(self.index).chain(self.neighbors()).collect();
        self.entries.retain(|index, _| keep.contains(index));
        self.prepare(self.index);
        if let Some(Entry::Ready(encoded)) = self.entries.get(&self.index) {
            self.shown = Some(encoded.protocol.clone());
            for neighbor in self.neighbors() {
                self.prepare(neighbor);
            }
        }
    }

    /// The images before and after the current one.
    fn neighbors(&self) -> Vec<usize> {
        let count = self.images.len();
        let mut neighbors = vec![(self.index + 1) % count, (self.index + count - 1) % count];
        neighbors.retain(|&index| index != self.index);
        neighbors.dedup();
        neighbors
    }

    /// Starts preparing image `index`, unless it is already.
    fn prepare(&mut self, index: usize) {
        if !self.entries.contains_key(&index) {
            let entry = spawn_job(&self.picker, Source::File(self.images[index].clone()), &self.area);
            self.entries.insert(index, entry);
        }
    }
}


/// `F3`: opens `Mode::ImagePreview` on the selected entry if it's a
/// supported image, else a no-op, among the active panel's images in its
/// own order. That panel stays active and is drawn beside the image, its
/// cursor following it (`handle_image_preview_key`); it used to switch to
/// the right panel, which was active after closing.
pub fn open_preview(app: &mut App) {
    let panel = app.active_panel();
    let Some(path) = panel.selected_path() else {
        return;
    };
    let paths = panel.entries.iter().filter(|entry| !entry.is_dir).map(|entry| panel.path.join(&entry.name)).collect();

    let Some(state) = ImagePreviewState::open_among(&app.image_picker, paths, &path) else {
        return;
    };
    app.mode = Mode::ImagePreview(state);
}


/// `Left`/`Right` cycle images, the panel's cursor following; `Esc` or
/// `F3` close the preview, the cursor on the image last shown.
pub fn handle_image_preview_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Mode::ImagePreview(state) = &mut app.mode else {
        return Ok(Effect::None);
    };

    match key.code {
        KeyCode::Left => state.prev(),
        KeyCode::Right => state.next(),
        KeyCode::Esc | KeyCode::F(3) => app.mode = Mode::Browsing,
        _ => return Ok(Effect::None),
    }
    if let Mode::ImagePreview(state) = &app.mode {
        let name = state.current_path().file_name().map(|name| name.to_string_lossy().into_owned());
        if let Some(name) = name {
            app.active_panel().select_name(&name);
        }
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
