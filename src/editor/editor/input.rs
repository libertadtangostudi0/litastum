use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::actions::{DeleteSelection, Execute, InsertChar};
use edtui::EditorMode;

use super::super::bindings::{
    anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    is_selection_consuming_key, wrap_line_boundary_arrow_movement,
};
use super::super::keymap_mode::EditorKeymapMode;
use super::Editor;

/// Whether `code` could plausibly have changed `state.lines` --
/// deliberately conservative (defaults to `true`, "might have
/// mutated") for anything not on this short, confirmed-safe list.
/// Checked regardless of modifiers: per `bindings/mod.rs`'s own key
/// table, `Shift` on any of these only ever extends a selection and
/// `Ctrl` only ever changes the jump granularity (word-wise, half-page)
/// -- neither ever reaches an `Insert`/`Delete`/paste action on this
/// list's own keys. See `Editor::dirty`'s own doc comment for why this
/// distinction exists at all.
///
/// `keymap_mode`/`mode_before` (the mode this keypress was actually
/// interpreted under -- captured *before* `event_handler.on_key_event`
/// ran) extend the same exemption to Vim's own `h`/`j`/`k`/`l`, the
/// exact Normal/Visual-mode equivalents of the arrow keys above
/// (`edtui`'s own `vim_keybindings()` binds both to the identical
/// `MoveBackward`/`MoveForward`/`MoveUp`/`MoveDown` actions -- confirmed
/// directly from its source). Found by hand while testing Vim mode
/// against the same pathologically-long-line file the arrow-key
/// exemption above was originally built for: without this, a Vim user
/// navigating with `hjkl` (Vim's own primary convention, not arrows)
/// got none of that fix's benefit, since these are plain `Char` keys,
/// not in the arrow-key list.
///
/// Deliberately doesn't extend this to Vim's other single-key motions
/// (`w`/`b`/`e`/`0`/`$`/...) -- `w` in particular is reused as the
/// *second* key of `dw`/`cw` (delete/change word forward), both of
/// which genuinely mutate the buffer; a plain keycode check run one key
/// at a time, with no visibility into `edtui`'s own pending multi-key
/// lookup state, can't safely tell "bare `w` navigating" apart from "`w`
/// completing `dw`". `h`/`j`/`k`/`l` were checked directly against the
/// full `vim_keybindings()` table and confirmed to never appear as a
/// component of any multi-key sequence at all, which is exactly why
/// only these four are safe to add here.
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


/// `input`'s own gate for whether a key (about to be dispatched, under
/// `Standard` only -- see `input`'s own doc comment) should get its own
/// `undo_stack` entry first. Builds on `can_mutate_buffer` (the exact
/// same "could this plausibly change the buffer" check `dirty`-tracking
/// already relies on, so navigation keys never push a pointless
/// snapshot) with one further exclusion: `Ctrl+C` (copy) genuinely never
/// mutates anything, but `can_mutate_buffer` alone can't tell that --
/// it isn't in the navigation list, so without this it would push a
/// real snapshot for a key that changes nothing, making the very next
/// `Ctrl+Z` a silent no-op restore into the state that key started in.
/// Deliberately *not* implemented by comparing the buffer before and
/// after the key runs instead (which would catch every no-op key, not
/// just this one) -- that's exactly the O(buffer length) comparison
/// `Editor::dirty`'s own doc comment already explains this app moved
/// away from paying on every keystroke.
fn should_capture_undo_snapshot(mode_before: EditorMode, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        return false;
    }
    can_mutate_buffer(EditorKeymapMode::Standard, mode_before, code)
}


impl Editor {
    /// Feeds one key event to the editor. Standard (non-modal) editing
    /// bindings — see `standard_key_handler` — everything not bound
    /// there that's a plain character still inserts, since the editor
    /// stays in `EditorMode::Insert` outside an active selection.
    ///
    /// Post-table correction passes run in sequence, all only ever
    /// *adding* behavior the table's own declarative chaining couldn't
    /// express on its own (see each one's own doc comment for why):
    ///
    /// 0. `is_selection_consuming_key` -- only for the five visual-mode
    ///    bindings (`Backspace`/`Delete`/`Ctrl+C`/`Ctrl+X`/`Ctrl+V`) that
    ///    consume an active selection. Resets `state.selection`/`state.mode`
    ///    back to plain typing by direct field assignment rather than
    ///    `edtui`'s own `SwitchMode(Insert)` -- see `is_selection_consuming_key`'s
    ///    own doc comment for the real bug this avoids (a redundant undo
    ///    checkpoint that made `Ctrl+Z` need two presses instead of one).
    /// 1. `anchor_fresh_shift_selection` -- only when this key just
    ///    transitioned a fresh selection into `Visual` mode (a plain,
    ///    non-word-select `Shift+Left`/`Right` with nothing already
    ///    selected -- `Shift+Up`/`Down` don't need this, see that
    ///    function's own doc comment for why). If the cell it anchored
    ///    on holds a real character, this is already exactly the wanted
    ///    one-character selection and nothing more happens; otherwise it
    ///    falls back to performing the actual move.
    /// 2. `wrap_line_boundary_arrow_movement` -- adds a row change when
    ///    the table's own handling of a plain/shifted `Left`/`Right`
    ///    turned out to be a no-op at a line boundary. Skipped entirely
    ///    when step 1 just deliberately left the cursor unmoved on a
    ///    real character -- that zero movement is a correct, intentional
    ///    stop (a fresh selection is exactly one character), not a
    ///    signal that a plain arrow press hit a wall, and treating it as
    ///    one would wrongly wrap an ordinary mid-line `Shift+Right` down
    ///    into the next line.
    /// 3. `exclude_landing_column_on_fresh_vertical_selection` -- only
    ///    when this key just opened a fresh `Shift+Up`/`Down` selection.
    ///    Trims the aligned landing column out of it (see that
    ///    function's own doc comment for the real report), and records
    ///    the pre-trim column into `vertical_shift_anchor_col` first, so
    ///    step 4 below can restore it later.
    /// 4. `close_selection_if_back_on_the_anchors_row` -- runs
    ///    unconditionally for every `Shift+Up`/`Down` press, fresh or
    ///    continuing. Closes the selection entirely once the cursor
    ///    lands back on the exact row it started a vertical excursion
    ///    from, since `MoveUp`/`MoveDown` never touch the column, so
    ///    that always means landing back on the anchor exactly --
    ///    without this, a `Shift+Down`+`Shift+Up` round trip (or the
    ///    reverse) would leave a phantom one-character selection instead
    ///    of returning to nothing, since `edtui`'s inclusive model can't
    ///    represent a zero-width selection on its own. Also restores
    ///    `state.cursor.col` from `vertical_shift_anchor_col` (step 3's
    ///    tracked value) at the same time -- without that, step 3's own
    ///    trim would leave the cursor permanently one column short of
    ///    where the excursion actually started.
    ///
    /// Every other key (and every already-working press these don't
    /// apply to) passes through completely unaffected.
    ///
    /// **Owns its own undo/redo entirely for `Standard`** (`undo_stack`/
    /// `redo_stack`, see the `undo` module) -- `Ctrl+Z`/`Ctrl+Y` are
    /// intercepted here, ahead of `edtui`'s own dispatch, and every
    /// other key that could plausibly mutate the buffer
    /// (`should_capture_undo_snapshot`) pushes its own snapshot *before*
    /// running. `.claude/rules/litastum-editor-undo.md` has the full
    /// history of why (`EditorState::capture()` is `pub(crate)`, so a
    /// fast paste could never register a boundary on `edtui`'s own
    /// stack, and undo through it fell back to per-character steps).
    /// `Vim` is completely unaffected -- `undo_stack`/`redo_stack` simply
    /// stay empty there, and `edtui`'s own real `capture_on_insert`/
    /// `Undo`/`Redo` mechanism keeps handling it exactly as it always
    /// has, same "no correction pass runs for Vim" rule every other part
    /// of this function follows.
    pub fn input(&mut self, key: KeyEvent) {
        let cursor_before = self.state.cursor;
        let mode_before = self.state.mode;

        if self.keymap_mode == EditorKeymapMode::Standard && self.handle_standard_key_ahead_of_dispatch(key, mode_before) {
            return;
        }

        self.event_handler.on_key_event(key, &mut self.state);

        // Every correction pass below is specifically tuned against
        // `Standard`'s own declarative table (`bindings::standard_key_handler`)
        // -- see `EditorKeymapMode::Vim`'s own doc comment for why none
        // of it runs against `edtui`'s own `vim_mode()` binding table
        // instead: Vim's modal, multi-key sequences were never
        // considered when these were written, and there's no reason to
        // assume they'd interact safely.
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

        // `Ctrl+V` -- see `fast_paste_from_clipboard`'s own doc
        // comment for why plain `PasteBefore` isn't used any more.
        // Vim's own table doesn't bind `Ctrl+V` to paste at all
        // (it's a real vim binding for visual-block mode instead),
        // which is why this whole function is `Standard`-only.
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

        // Typing a plain character over an active selection should
        // replace it -- reported directly as a real gap: select-all
        // (`Ctrl+A`) then typing a character left the selection
        // completely untouched and the character never appeared at
        // all. Root cause: a plain `Char` has no binding at all for
        // `Visual` mode in `standard_key_handler`'s own table (only
        // `Backspace`/`Delete`/`Ctrl+C`/`X`/`V` are bound there --
        // `is_selection_consuming_key`'s own list), and `edtui`'s own
        // built-in "typing inserts" fallback only fires in `Insert`
        // mode, never `Visual` -- so the keypress reached neither path
        // and was silently dropped. Handled directly here, ahead of
        // dispatch, rather than added to `is_selection_consuming_key`'s
        // list: that list only ever *clears* the selection after
        // `edtui`'s own dispatch already ran, which works for
        // `Backspace`/`Delete`/paste (each has a real `Visual`-mode
        // binding of its own that already deletes/replaces something),
        // but a plain `Char` has no such binding to piggyback on. The
        // snapshot above already covers this key (a plain `Char`), so
        // this only needs to perform the actual mutation.
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
