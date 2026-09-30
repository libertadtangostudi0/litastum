use serde::{Deserialize, Serialize};

/// Which key-binding scheme a built-in editor session uses --
/// user-selectable via the editor's own **F9** menu (`keymap_menu.rs`,
/// distinct from the browsing screen's own F9 -> `theming::MainMenu`),
/// requested directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EditorKeymapMode {
    /// Our non-modal keymap (`bindings::standard_key_handler`) plus every
    /// correction pass `Editor::input` layers on it -- all tuned for this
    /// table; none run under `Vim`.
    #[default]
    Standard,
    /// `edtui`'s own `EditorEventHandler::vim_mode()`, as-is. Our correction
    /// passes are skipped: they were never tuned for Vim's modal, multi-key
    /// sequences.
    Vim,
}

impl EditorKeymapMode {
    /// Display label for the editor's own F9 picker.
    pub fn label(self) -> &'static str {
        match self {
            EditorKeymapMode::Standard => "Standard",
            EditorKeymapMode::Vim => "Vim",
        }
    }

    /// Every mode, in the order the picker lists them.
    pub fn all() -> [EditorKeymapMode; 2] {
        [EditorKeymapMode::Standard, EditorKeymapMode::Vim]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_standard() {
        assert_eq!(EditorKeymapMode::default(), EditorKeymapMode::Standard);
    }

    #[test]
    fn all_lists_both_modes() {
        assert_eq!(EditorKeymapMode::all(), [EditorKeymapMode::Standard, EditorKeymapMode::Vim]);
    }

    #[test]
    fn round_trips_through_json() {
        for mode in EditorKeymapMode::all() {
            let json = serde_json::to_string(&mode).unwrap();
            let back: EditorKeymapMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, mode);
        }
    }
}
