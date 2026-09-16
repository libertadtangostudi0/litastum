use serde::{Deserialize, Serialize};

/// Which key-binding scheme a built-in editor session uses --
/// user-selectable via the editor's own **F9** menu (`keymap_menu.rs`,
/// distinct from the browsing screen's own F9 -> `theming::MainMenu`),
/// requested directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EditorKeymapMode {
    /// This project's own non-modal (VSCode-convention) keymap
    /// (`bindings::standard_key_handler`), plus every hand-rolled
    /// correction pass `Editor::input` layers on top of it (word-wise
    /// selection touch-tracking, `Shift`+arrow anchor fixes, line-
    /// boundary wrapping, ...) -- see `.claude/rules/litastum-stack.md`
    /// for the long history of why those exist. All of that is
    /// specifically tuned against this keymap's own declarative table;
    /// none of it runs while `Vim` (below) is active instead.
    #[default]
    Standard,
    /// `edtui`'s own bundled `EditorEventHandler::vim_mode()`, used
    /// as-is -- implementing real Vim keybindings from scratch is well
    /// outside this project's own scope, and `edtui` already ships one.
    /// This project's own correction passes (see `Standard`'s own doc
    /// comment) are specifically tuned against `Standard`'s keymap and
    /// are skipped entirely while this is active instead -- running them
    /// against Vim's own modal, multi-key sequences isn't guaranteed
    /// safe and hasn't been attempted.
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
