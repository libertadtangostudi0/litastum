//! Keys: the window's own first (tabs, full screen, zoom, paste), the
//! rest to the shown tab's litastum.

use std::time::Instant;

use alacritty_terminal::term::TermMode;
use winit::event::ElementState;

use super::App;
use crate::input;

impl App {
    /// A key press or release. The window's own keys come first: tabs,
    /// full screen, zoom, paste. `Ctrl+V`/`Shift+Insert` paste the
    /// clipboard as typed text, as Windows Terminal does: the key itself
    /// never reaches litastum, which reads the clipboard on that key
    /// (`windows_terminal::paste_hotkey`) and swallows the typed copy.
    pub(super) fn key(&mut self, event: winit::event::KeyEvent) {
        let pressed = event.state == ElementState::Pressed;
        if pressed {
            // Typing keeps the cursor steady, as in other terminals.
            self.cursor_shown = true;
            self.next_blink = Instant::now() + super::BLINK_INTERVAL;
        }
        if let Some(chord) = input::tab_chord(event.physical_key, self.modifiers) {
            if pressed {
                self.tab_chord(chord);
            }
            return;
        }
        if input::is_fullscreen_key(event.physical_key, self.modifiers) {
            if pressed && !event.repeat {
                self.toggle_fullscreen();
            }
            return;
        }
        if let Some(zoom) = input::zoom_chord(event.physical_key, self.modifiers) {
            if pressed {
                self.zoom(zoom);
            }
            return;
        }
        if input::is_paste_chord(&event.logical_key, event.physical_key, self.modifiers) {
            if pressed {
                self.paste();
            }
            return;
        }
        if pressed && !self.modifiers.control_key() && input::needs_plain_text(event.text.as_deref()) {
            self.write(event.text.as_deref().unwrap_or_default());
            return;
        }
        #[cfg(windows)]
        {
            use winit::platform::scancode::PhysicalKeyExtScancode;
            let scan_code = event.physical_key.to_scancode().map_or(0, |code| (code & 0xFF) as u16);
            if let Some(record) = crate::win32_input::key_record(&event.logical_key, event.physical_key, event.text.as_deref(), self.modifiers, pressed, scan_code) {
                self.write(record.sequence());
            }
        }
        #[cfg(not(windows))]
        if pressed {
            let app_cursor = self.mode().contains(TermMode::APP_CURSOR);
            if let Some(bytes) = crate::keys::encode(&event.logical_key, event.physical_key, event.text.as_deref(), self.modifiers, app_cursor) {
                self.write(bytes);
            }
        }
    }

    /// The clipboard's text as input. Bracketed on Unix when the program
    /// asked for it; ConPTY has no use for the brackets.
    fn paste(&self) {
        let Ok(text) = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) else {
            return;
        };
        let bracketed = cfg!(not(windows)) && self.mode().contains(TermMode::BRACKETED_PASTE);
        self.write(input::paste_bytes(&text, bracketed));
    }
}
