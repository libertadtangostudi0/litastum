use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Rewrites `key` so a `Ctrl+<letter>` chord typed under a non-Latin
/// keyboard layout still matches this app's own (Latin-only) bindings.
///
/// Reported directly, traced through the real log: `Ctrl+C` in the
/// built-in editor did nothing at all, and `logs/litastum.log` showed
/// exactly why -- `editor key key=KeyEvent { code: Char('с'), modifiers:
/// KeyModifiers(CONTROL), ... }`. That `'с'` is Cyrillic (U+0441), not
/// Latin `c` (U+0063) -- on a Russian (ЙЦУКЕН) keyboard layout, Windows
/// still translates the physical `C` key's virtual-key code through the
/// *active* layout even while `Ctrl` is held, so `crossterm` reports
/// whatever letter that layout puts there, not the Latin one every
/// `KeyCode::Char('c')`-shaped binding in this app (`Copy`, `Paste`,
/// `Save`, `Find`, ...) is written against. This isn't specific to the
/// editor or to Cyrillic -- every `Ctrl+<letter>` binding in the app
/// (the always-live command line's `Ctrl+O`/`Ctrl+P`/`Ctrl+U`, the
/// transfer popup's `Ctrl+C`/`X`/`V`, ...) reads the same raw
/// `KeyEvent` and would break the same way under *any* non-Latin
/// layout, not just a Russian one.
///
/// Terminals on Unix don't have this problem at all -- `Ctrl+<letter>`
/// there is a real control *byte* (`0x01`-`0x1A`), computed from the
/// physical key position by the terminal driver itself, never
/// layout-translated. This is a Windows Console-specific quirk
/// (`ReadConsoleInputW` still runs the active layout's virtual-key-to-
/// character translation for every key, `Ctrl` held or not), which is
/// why the fix lives here rather than needing `#[cfg(windows)]` gating
/// with a Unix branch of its own -- on Unix, no `LAYOUTS` table below
/// ever matches what a real terminal sends for `Ctrl+<letter>`, so this
/// is a harmless no-op there.
///
/// **Architecture, for adding another layout**: doesn't special-case
/// Cyrillic specifically -- `layouts::LAYOUTS` is a plain list of
/// per-layout position tables (`layouts::LayoutTable`), each mapping
/// that layout's own letters to whichever Latin letter sits in the same
/// physical key position on a standard US QWERTY layout. This function
/// just searches every table in the list for the typed character;
/// there's no notion of "the currently active layout" to track or
/// switch on (crossterm never reports which layout produced a
/// character, only the character itself) -- a real OS layout switch
/// just means a different table happens to match, which is also why
/// this stays correct even if the user switches layouts mid-session.
/// Supporting one more layout (French AZERTY, German QWERTZ, ...) is
/// purely additive: write its own table in `layouts.rs` and list it in
/// `LAYOUTS`, nothing here or at either call site needs to change.
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
