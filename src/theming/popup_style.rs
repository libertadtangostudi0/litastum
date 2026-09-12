use serde::{Deserialize, Serialize};

/// Which chrome flavor popups render with -- user-selectable via
/// **F9 -> Options -> UI**, kept as two permanent, coexisting options
/// rather than converging on one: an explicit request to keep the
/// original square-cornered look (`Classic`) alongside the newer
/// card-style one (`Rounded`, `ui/popup.rs`) rather than migrating
/// every popup onto whichever style review happened to favor. See
/// `.claude/rules/litastum-popup-design.md` for `Rounded`'s own settled
/// fill/corner-glyph history -- that decision is about `Rounded`'s own
/// look, not about whether `Classic` should keep existing alongside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PopupStyle {
    /// Plain square `Block::borders(ALL)`, title baked directly into
    /// the border, no interior padding -- the look every popup in this
    /// app used before `ui/popup.rs` existed, and what `ui/find_file.rs`
    /// still looks like today.
    Classic,
    /// `BorderType::Rounded`, uniform padding, title drawn as the
    /// frame's own first content line -- `ui/popup.rs::draw_frame`.
    #[default]
    Rounded,
}

impl PopupStyle {
    /// Display label for the F9 -> UI picker.
    pub fn label(self) -> &'static str {
        match self {
            PopupStyle::Classic => "Classic",
            PopupStyle::Rounded => "Rounded",
        }
    }

    /// Every style, in the order the picker lists them.
    pub fn all() -> [PopupStyle; 2] {
        [PopupStyle::Classic, PopupStyle::Rounded]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_rounded() {
        assert_eq!(PopupStyle::default(), PopupStyle::Rounded);
    }

    #[test]
    fn all_lists_both_styles() {
        assert_eq!(PopupStyle::all(), [PopupStyle::Classic, PopupStyle::Rounded]);
    }

    #[test]
    fn round_trips_through_json() {
        for style in PopupStyle::all() {
            let json = serde_json::to_string(&style).unwrap();
            let back: PopupStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(back, style);
        }
    }
}
