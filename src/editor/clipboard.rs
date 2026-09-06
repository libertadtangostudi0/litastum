use arboard::Clipboard as OsClipboard;
use edtui::clipboard::ClipboardTrait;
use tracing::{debug, warn};

/// Bridges `edtui`'s pluggable clipboard trait to the real OS clipboard
/// via our own minimal `arboard` dependency (`default-features =
/// false`, so no `image` crate) — rather than enabling `edtui`'s own
/// `arboard` feature, which pulls `image`/`image-data` in for bitmap
/// clipboard support we don't need.
pub(super) struct OsClipboardBridge;

impl ClipboardTrait for OsClipboardBridge {
    fn set_text(&mut self, text: String) {
        match OsClipboard::new() {
            Ok(mut clipboard) => match clipboard.set_text(text) {
                Ok(()) => debug!("clipboard: set ok"),
                Err(err) => warn!(%err, "clipboard: set_text failed"),
            },
            Err(err) => warn!(%err, "clipboard: unavailable (Clipboard::new failed)"),
        }
    }

    fn get_text(&mut self) -> String {
        match OsClipboard::new() {
            Ok(mut clipboard) => clipboard.get_text().unwrap_or_default(),
            Err(err) => {
                warn!(%err, "clipboard: unavailable (Clipboard::new failed)");
                String::new()
            }
        }
    }
}
