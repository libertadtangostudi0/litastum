use std::path::PathBuf;

/// Which half of the popup is showing — the typed query, or the
/// results it produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindFilePhase {
    Typing,
    Results,
}


pub struct FindFileState {
    pub phase: FindFilePhase,
    pub query: String,
    /// Character index into `query` — see `text_field.rs`.
    pub cursor: usize,
    pub results: Vec<PathBuf>,
    pub selected: usize,
    /// Feedback from the last `Ctrl+S` export, shown under the results
    /// list until the next export attempt (success or failure — both
    /// get a message, there's no other status-bar surface to put this
    /// on yet). `(label, detail)` — `("Exported to:", "<path>")` or
    /// `("Export failed:", "<error>")` — rendered on two separate
    /// lines rather than one, since a real Downloads path is easily
    /// wide enough to blow past the popup's width on one line.
    pub export_message: Option<(String, String)>,
}


impl FindFileState {
    pub fn new() -> Self {
        Self {
            phase: FindFilePhase::Typing,
            query: String::new(),
            cursor: 0,
            results: Vec::new(),
            selected: 0,
            export_message: None,
        }
    }
}
