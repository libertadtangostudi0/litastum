use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::actions::{DeleteSelection, Execute, InsertChar};
use edtui::EditorMode;

use super::super::bindings::{
    anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    is_selection_consuming_key, wrap_line_boundary_arrow_movement,
};
use super::super::keymap_mode::EditorKeymapMode;
use super::Editor;

/// Whether `code` could have changed `state.lines` -- `true` for anything
/// not on a short, confirmed-safe navigation list (any modifiers: `Shift`
/// only selects, `Ctrl` only changes the jump size). In Vim's Normal/
/// Visual mode `h`/`j`/`k`/`l` count as navigation too; `w`/`b`/`e`/...
/// don't, since `w` also completes `dw`/`cw`. Lets `Editor::dirty` skip
/// its full-buffer comparison for pure navigation. History: docs/history/editor-keymap.md.
fn can_mutate_buffer(keymap_mode: EditorKeymapMode, mode_before: EditorMode, code: KeyCode) -> bool {
    if matches!(
        code,
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown
    ) {
        return false;
    }
    if keymap_mode == EditorKeymapMode::Vim
        && matches!(mode_before, EditorMode::Normal | EditorMode::Visual)
        && matches!(code, KeyCode::Char('h' | 'j' | 'k' | 'l'))
    {
        return false;
    }
    true
}


/// Whether a key gets an undo snapshot before it runs (`Standard` only):
/// `can_mutate_buffer` minus `Ctrl+C`, which never mutates -- a snapshot
/// for it would make the next `Ctrl+Z` a no-op. Not a before/after
/// buffer comparison, which would cost O(buffer) per key.
fn should_capture_undo_snapshot(mode_before: EditorMode, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        return false;
    }
    can_mutate_buffer(EditorKeymapMode::Standard, mode_before, code)
}


impl Editor {
    /// Feeds one key event to the editor. For the `Standard` keymap,
    /// correction passes run around `edtui`'s own dispatch, each adding
    /// what the declarative table can't express:
    ///
    /// 0. `is_selection_consuming_key` -- after `Backspace`/`Delete`/
    ///    `Ctrl+C/X/V` on a selection, back to plain typing by field
    ///    assignment (`SwitchMode(Insert)` would add an undo checkpoint).
    /// 1. `anchor_fresh_shift_selection` -- a fresh `Shift+Left/Right`
    ///    selects exactly one character.
    /// 2. `wrap_line_boundary_arrow_movement` -- `Left`/`Right` that
    ///    didn't move at a line boundary crosses it; skipped when step 1
    ///    stopped on a character on purpose.
    /// 3. `exclude_landing_column_on_fresh_vertical_selection` -- a
    ///    fresh `Shift+Up/Down` doesn't select the aligned landing
    ///    column; the pre-trim column goes to `vertical_shift_anchor_col`.
    /// 4. `close_selection_if_back_on_the_anchors_row` -- every
    ///    `Shift+Up/Down`: back on the anchor's row closes the selection
    ///    and restores that column.
    ///
    /// `Standard` also owns undo/redo here (`Ctrl+Z`/`Ctrl+Y` intercepted,
    /// a snapshot before each mutating key -- `edtui`'s `capture()` is
    /// unreachable). `Vim` gets none of this: `edtui`'s own table, undo
    /// and modes, untouched. History: docs/history/shift-select.md,
    /// docs/history/editor-undo.md.
    pub fn input(&mut self, key: KeyEvent) {
        let cursor_before = self.state.cursor;
        let mode_before = self.state.mode;

        if self.keymap_mode == EditorKeymapMode::Standard && self.handle_standard_key_ahead_of_dispatch(key, mode_before) {
            return;
        }

        self.event_handler.on_key_event(key, &mut self.state);

        // The correction passes are tuned for `Standard`'s table; Vim's
        // modal multi-key sequences were never considered.
        if self.keymap_mode == EditorKeymapMode::Standard {
            if mode_before == EditorMode::Visual && is_selection_consuming_key(&key) {
                self.state.selection = None;
                self.state.mode = EditorMode::Insert;
            }

            let freshly_entered_visual = mode_before != EditorMode::Visual && self.state.mode == EditorMode::Visual;
            let anchored_on_a_real_character =
                freshly_entered_visual && anchor_fresh_shift_selection(&mut self.state, key.code, cursor_before);

            if !anchored_on_a_real_character {
                wrap_line_boundary_arrow_movement(&mut self.state, key.code, key.modifiers, cursor_before);
            }

            if freshly_entered_visual && matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.vertical_shift_anchor_col = Some(cursor_before.col);
                exclude_landing_column_on_fresh_vertical_selection(&mut self.state, key.code);
            }

            close_selection_if_back_on_the_anchors_row(&mut self.state, key.code, &mut self.vertical_shift_anchor_col);
        }

        if can_mutate_buffer(self.keymap_mode, mode_before, key.code) {
            self.buffer_changed();
        }
    }

    /// `Standard`-only handling that has to run *before* `edtui`'s own
    /// dispatch: fast paste, this app's own undo/redo, the undo snapshot
    /// for every other mutating key, and typing over a selection.
    /// Returns `true` when the key was fully handled here and `input`
    /// should stop.
    fn handle_standard_key_ahead_of_dispatch(&mut self, key: KeyEvent, mode_before: EditorMode) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Our fast paste, not `edtui`'s (`fast_paste_from_clipboard`).
        // Standard-only: in Vim `Ctrl+V` is visual-block mode.
        if ctrl && key.code == KeyCode::Char('v') {
            self.fast_paste_from_clipboard();
            return true;
        }
        if ctrl && key.code == KeyCode::Char('z') {
            if self.undo() {
                return true;
            }
        } else if ctrl && key.code == KeyCode::Char('y') {
            if self.redo() {
                return true;
            }
        } else if should_capture_undo_snapshot(mode_before, key.code, key.modifiers) {
            self.push_undo_snapshot();
        }

        // Typing over a selection replaces it. A plain `Char` has no
        // `Visual`-mode binding and `edtui` only inserts in Insert mode,
        // so without this the key was dropped. The snapshot above already
        // covers it. History: docs/history/editor-keymap.md.
        if self.state.mode == EditorMode::Visual {
            if let KeyCode::Char(c) = key.code {
                if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) {
                    DeleteSelection.execute(&mut self.state);
                    InsertChar(c).execute(&mut self.state);
                    self.state.mode = EditorMode::Insert;
                    self.buffer_changed();
                    return true;
                }
            }
        }

        false
    }
}
