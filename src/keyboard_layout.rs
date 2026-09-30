use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Rewrites a `Ctrl+<letter>` typed under a non-Latin layout to the Latin
/// letter on the same key, so it matches our bindings. The Windows console
/// translates the key through the active layout even with `Ctrl` held
/// (`Ctrl+C` arrived as Cyrillic U+0441), which broke every `Ctrl+<letter>`
/// binding. Unix terminals send control bytes instead, so this is a no-op
/// there and needs no `cfg`.
///
/// `layouts::LAYOUTS` is a list of per-layout position tables; every table
/// is searched, with no notion of the active layout (`crossterm` doesn't
/// report it), so switching layouts mid-session just works. Adding a
/// layout means adding a table. History: docs/history/keyboard-layout.md.
pub(crate) fn normalize_ctrl_shortcut(mut key: KeyEvent) -> KeyEvent {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return key;
    }
    if let KeyCode::Char(c) = key.code {
        let is_upper = c.is_uppercase();
        let lower = c.to_lowercase().next().unwrap_or(c);
        if let Some(latin) = layouts::latin_by_position(lower) {
            key.code = KeyCode::Char(if is_upper { latin.to_ascii_uppercase() } else { latin });
        }
    }
    key
}

mod layouts;


#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_char(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn remaps_the_actual_reported_case_ctrl_c() {
        let normalized = normalize_ctrl_shortcut(ctrl_char('с'));
        assert_eq!(normalized.code, KeyCode::Char('c'));
        assert_eq!(normalized.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn preserves_uppercase() {
        let normalized = normalize_ctrl_shortcut(ctrl_char('С'));
        assert_eq!(normalized.code, KeyCode::Char('C'));
    }

    #[test]
    fn leaves_a_latin_letter_untouched() {
        let normalized = normalize_ctrl_shortcut(ctrl_char('c'));
        assert_eq!(normalized.code, KeyCode::Char('c'));
    }

    #[test]
    fn leaves_a_non_ctrl_key_untouched() {
        let key = KeyEvent::new(KeyCode::Char('с'), KeyModifiers::NONE);
        assert_eq!(normalize_ctrl_shortcut(key).code, KeyCode::Char('с'), "plain typing must never be remapped, only Ctrl chords");
    }

    #[test]
    fn leaves_a_non_letter_key_untouched() {
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(normalize_ctrl_shortcut(key).code, KeyCode::Left);
    }

    #[test]
    fn covers_every_bound_ctrl_shortcut_in_this_app() {
        // The actual letters real Ctrl+<letter> bindings exist for
        // today (editor Save/Find/SelectAll/Undo/Redo/Copy/Cut/Paste,
        // command line Ctrl+O/P/U, the transfer popup's Copy/Cut/Paste)
        // -- every one of these must resolve to its own Latin letter
        // when typed as its ЙЦУКЕН position.
        let cases = [
            ('с', 'c'), // Copy
            ('ч', 'x'), // Cut
            ('м', 'v'), // Paste
            ('ы', 's'), // Save
            ('а', 'f'), // Find
            ('ф', 'a'), // SelectAll
            ('я', 'z'), // Undo
            ('н', 'y'), // Redo
            ('щ', 'o'), // Ctrl+O (show/hide panels)
            ('з', 'p'), // Ctrl+P (shell picker)
            ('г', 'u'), // Ctrl+U (swap panels)
        ];
        for (cyrillic, latin) in cases {
            assert_eq!(normalize_ctrl_shortcut(ctrl_char(cyrillic)).code, KeyCode::Char(latin), "Ctrl+{cyrillic} should have normalized to Ctrl+{latin}");
        }
    }
}
