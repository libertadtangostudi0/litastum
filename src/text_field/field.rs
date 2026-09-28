use crossterm::event::KeyEvent;

use super::EditOutcome;

/// A single-line text field: the text, the cursor (a character index)
/// and the selection anchor (`None` = no selection), kept together so a
/// caller can't reset one and forget the others. Every method keeps
/// `cursor <= text.chars().count()`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}


impl TextField {
    pub fn new() -> Self {
        Self::default()
    }


    /// A field holding `text`, cursor at the end, nothing selected.
    pub fn with_text(text: impl Into<String>) -> Self {
        let mut field = Self::new();
        field.set_text(text);
        field
    }


    /// A field holding `text` with the cursor at `cursor` (clamped to the
    /// end), nothing selected.
    pub fn with_cursor_at(text: impl Into<String>, cursor: usize) -> Self {
        let text = text.into();
        let cursor = cursor.min(text.chars().count());
        Self { text, cursor, anchor: None }
    }


    /// A field in an exact state -- for tests that start mid-edit.
    #[cfg(test)]
    pub fn at(text: impl Into<String>, cursor: usize, anchor: Option<usize>) -> Self {
        let text = text.into();
        assert!(cursor <= text.chars().count(), "cursor past the end of the text");
        Self { text, cursor, anchor }
    }


    pub fn text(&self) -> &str {
        &self.text
    }


    pub fn cursor(&self) -> usize {
        self.cursor
    }


    #[cfg(test)]
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }


    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }


    /// The selected character range, `start <= end`, if anything is
    /// selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.map(|anchor| super::selection_range(anchor, self.cursor))
    }


    /// Replaces the text, cursor at the end, nothing selected.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.move_to_end();
    }


    pub fn clear(&mut self) {
        self.set_text(String::new());
    }


    /// Cursor to the end, selection dropped.
    pub fn move_to_end(&mut self) {
        self.cursor = self.text.chars().count();
        self.anchor = None;
    }


    /// The standard single-line key layout (`super::apply_edit_key`).
    pub fn apply_key(&mut self, key: KeyEvent) -> EditOutcome {
        super::apply_edit_key(&mut self.text, &mut self.cursor, &mut self.anchor, key)
    }


    /// Types `c` at the cursor, replacing the selection if there is one.
    pub fn insert_char(&mut self, c: char) {
        super::delete_selection(&mut self.text, &mut self.cursor, &mut self.anchor);
        super::insert_char(&mut self.text, &mut self.cursor, c);
    }


    /// Deletes the selection, or else the character before the cursor.
    pub fn backspace(&mut self) {
        if !super::delete_selection(&mut self.text, &mut self.cursor, &mut self.anchor) {
            super::backspace(&mut self.text, &mut self.cursor);
        }
    }


    /// Deletes the selection, or else the character at the cursor.
    pub fn delete_forward(&mut self) {
        if !super::delete_selection(&mut self.text, &mut self.cursor, &mut self.anchor) {
            super::delete_forward(&mut self.text, &mut self.cursor);
        }
    }


    /// Appends `c` at the end regardless of the cursor -- for a popup
    /// that edits this field's text as a plain append-only filter.
    pub fn push_char(&mut self, c: char) {
        self.text.push(c);
        self.move_to_end();
    }


    /// Removes the last character regardless of the cursor -- the
    /// counterpart of `push_char`.
    pub fn pop_char(&mut self) {
        self.text.pop();
        self.move_to_end();
    }


    pub fn extend_selection_left(&mut self) {
        super::extend_selection_left(&mut self.cursor, &mut self.anchor);
    }


    pub fn extend_selection_right(&mut self) {
        super::extend_selection_right(&self.text, &mut self.cursor, &mut self.anchor);
    }


    pub fn extend_selection_word_left(&mut self) {
        super::extend_selection_word_left(&self.text, &mut self.cursor, &mut self.anchor);
    }


    pub fn extend_selection_word_right(&mut self) {
        super::extend_selection_word_right(&self.text, &mut self.cursor, &mut self.anchor);
    }


    /// Moves a word left, dropping the selection.
    pub fn move_word_left(&mut self) {
        self.anchor = None;
        super::move_word_left(&self.text, &mut self.cursor);
    }


    /// Moves a word right, dropping the selection.
    pub fn move_word_right(&mut self) {
        self.anchor = None;
        super::move_word_right(&self.text, &mut self.cursor);
    }


    /// Selects the whole text, cursor at the end.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.chars().count();
    }


    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }
}


#[cfg(test)]
mod tests {
    use super::TextField;

    #[test]
    fn with_text_puts_the_cursor_at_the_end() {
        let field = TextField::with_text("café");
        assert_eq!(field.cursor(), 4, "a character index, not a byte offset");
        assert_eq!(field.anchor(), None);
    }

    #[test]
    fn set_text_resets_cursor_and_selection_together() {
        let mut field = TextField::at("abc", 1, Some(3));
        field.set_text("xy");
        assert_eq!((field.text(), field.cursor(), field.anchor()), ("xy", 2, None));
    }

    #[test]
    fn clear_leaves_an_empty_field_with_nothing_selected() {
        let mut field = TextField::at("abc", 1, Some(3));
        field.clear();
        assert_eq!(field, TextField::new());
    }

    #[test]
    fn insert_char_replaces_the_selection() {
        let mut field = TextField::at("abcd", 3, Some(1));
        field.insert_char('x');
        assert_eq!((field.text(), field.cursor(), field.anchor()), ("axd", 2, None));
    }

    #[test]
    fn backspace_deletes_the_selection_before_a_single_character() {
        let mut field = TextField::at("abcd", 1, Some(3));
        field.backspace();
        assert_eq!((field.text(), field.cursor()), ("ad", 1));

        field.backspace();
        assert_eq!((field.text(), field.cursor()), ("d", 0));
    }

    #[test]
    fn delete_forward_deletes_the_selection_before_a_single_character() {
        let mut field = TextField::at("abcd", 3, Some(1));
        field.delete_forward();
        assert_eq!((field.text(), field.cursor()), ("ad", 1));

        field.delete_forward();
        assert_eq!((field.text(), field.cursor()), ("a", 1));
    }

    #[test]
    fn push_and_pop_work_at_the_end_whatever_the_cursor() {
        let mut field = TextField::at("di", 0, Some(1));
        field.push_char('r');
        assert_eq!((field.text(), field.cursor(), field.anchor()), ("dir", 3, None));

        field.pop_char();
        field.pop_char();
        field.pop_char();
        field.pop_char();
        assert_eq!((field.text(), field.cursor()), ("", 0), "popping an empty field is a no-op");
    }

    #[test]
    fn word_moves_drop_the_selection() {
        let mut field = TextField::at("one two", 7, Some(5));
        field.move_word_left();
        assert_eq!((field.cursor(), field.anchor()), (4, None));
    }

    #[test]
    fn selection_is_normalized_whichever_way_it_was_extended() {
        let mut field = TextField::with_text("abc");
        field.extend_selection_left();
        field.extend_selection_left();
        assert_eq!(field.selection(), Some((1, 3)));
    }
}
