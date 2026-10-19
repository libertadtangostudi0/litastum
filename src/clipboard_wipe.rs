//! The clipboard emptied a while after a password was pasted from it
//! (requested), as password managers do -- unless something else has
//! been copied since. Only a hash of the text is kept, never the text.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// How long a pasted password stays on the clipboard.
pub const WIPE_AFTER: Duration = Duration::from_secs(30);


/// A wipe waiting: when, and the hash of what it will wipe.
#[derive(Debug, Default)]
pub struct ClipboardWipe {
    due: Option<(Instant, u64)>,
}

impl ClipboardWipe {
    /// `text` was pasted at a password prompt at `now`.
    pub fn arm(&mut self, text: &str, now: Instant) {
        self.due = Some((now + WIPE_AFTER, digest(text)));
    }

    /// Whether a wipe waits -- the event loop then keeps polling.
    pub fn pending(&self) -> bool {
        self.due.is_some()
    }

    /// Empties the clipboard once due, if it still holds the pasted text.
    pub fn poll(&mut self, now: Instant) {
        let Some((at, pasted)) = self.due else {
            return;
        };
        if now < at {
            return;
        }
        self.due = None;
        if crate::text_field::clipboard::get().is_some_and(|text| digest(&text) == pasted) {
            crate::text_field::clipboard::set(String::new());
        }
    }
}


fn digest(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_field::clipboard;

    #[test]
    fn the_pasted_password_is_wiped_once_due() {
        clipboard::set("s3cr3t".to_string());
        let start = Instant::now();
        let mut wipe = ClipboardWipe::default();
        wipe.arm("s3cr3t", start);

        wipe.poll(start + WIPE_AFTER - Duration::from_secs(1));
        assert_eq!(clipboard::get().as_deref(), Some("s3cr3t"), "not yet");
        assert!(wipe.pending());

        wipe.poll(start + WIPE_AFTER);
        assert_eq!(clipboard::get().as_deref(), Some(""));
        assert!(!wipe.pending());
    }

    /// Something copied since stays.
    #[test]
    fn something_copied_since_is_left() {
        clipboard::set("s3cr3t".to_string());
        let start = Instant::now();
        let mut wipe = ClipboardWipe::default();
        wipe.arm("s3cr3t", start);
        clipboard::set("a path".to_string());

        wipe.poll(start + WIPE_AFTER);

        assert_eq!(clipboard::get().as_deref(), Some("a path"));
    }
}
