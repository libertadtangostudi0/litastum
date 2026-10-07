use edtui::actions::{Chainable, Execute, SwitchMode};
use edtui::{EditorMode, Index2, RowIndex};

use super::word_select_touch::WordSelectTouch;
use super::Editor;

impl Editor {
    /// `Ctrl+Down`/`Ctrl+Up` (`Standard`): the caret jumps between blocks
    /// of code -- runs of non-blank lines. Down: the first line of the
    /// next block (the last line if there is none). Up: the first line of
    /// the current block, or of the previous one from there or from a
    /// blank line. The caret lands on the line's first non-blank
    /// character; a selection is dropped, as by a plain arrow. Scans only
    /// the rows it passes.
    pub fn move_by_block(&mut self, forward: bool) {
        let row = self.state.cursor.row;
        let target = if forward { self.next_block_start(row) } else { self.block_start_above(row) };
        if self.state.selection.is_some() {
            SwitchMode(EditorMode::Normal).chain(SwitchMode(EditorMode::Insert)).execute(&mut self.state);
            self.vertical_shift_anchor = None;
            self.word_select_true_anchor = None;
            self.word_select_touch = WordSelectTouch::Untouched;
        }
        self.state.cursor = Index2::new(target, self.first_non_blank_column(target));
    }

    fn row_is_blank(&self, row: usize) -> bool {
        self.state.lines.get(RowIndex::new(row)).is_none_or(|chars| chars.iter().all(|c| c.is_whitespace()))
    }

    fn first_non_blank_column(&self, row: usize) -> usize {
        self.state.lines.get(RowIndex::new(row)).and_then(|chars| chars.iter().position(|c| !c.is_whitespace())).unwrap_or(0)
    }

    fn next_block_start(&self, row: usize) -> usize {
        let last = self.line_count().saturating_sub(1);
        let mut next = row;
        while next < last && !self.row_is_blank(next) {
            next += 1;
        }
        while next < last && self.row_is_blank(next) {
            next += 1;
        }
        // On the last line of the last block, `next` didn't move past it.
        if next == row { last } else { next }
    }

    fn block_start_above(&self, row: usize) -> usize {
        if row == 0 {
            return 0;
        }
        let mut up = row;
        // Not inside a block (on its first line, or on a blank line): past
        // the blank lines above, onto the previous block's last line.
        if self.row_is_blank(row) || self.row_is_blank(row - 1) {
            up = row - 1;
            while up > 0 && self.row_is_blank(up) {
                up -= 1;
            }
        }
        while up > 0 && !self.row_is_blank(up - 1) {
            up -= 1;
        }
        up
    }
}


#[cfg(test)]
mod tests {
    use edtui::Index2;

    use super::Editor;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::unique_scratch_dir;

    fn open_text(text: &str) -> Editor {
        let path = unique_scratch_dir("block-move").join("code.rs");
        std::fs::write(&path, text).unwrap();
        Editor::open(path, None, EditorKeymapMode::Standard).unwrap()
    }

    const CODE: &str = "fn a() {\n    one();\n}\n\n\n    fn b() {\n    two();\n}\n\nlast\n";

    #[test]
    fn ctrl_down_steps_to_each_next_block_start() {
        let mut editor = open_text(CODE);

        editor.move_by_block(true);
        assert_eq!(editor.cursor(), Index2::new(5, 4), "the next block's first line, on its first non-blank character");
        editor.move_by_block(true);
        assert_eq!(editor.cursor(), Index2::new(9, 0));
        editor.move_by_block(true);
        assert_eq!(editor.cursor().row, editor.line_count() - 1, "no block below: the last line");
    }

    #[test]
    fn ctrl_up_goes_to_this_blocks_start_then_the_previous_ones() {
        let mut editor = open_text(CODE);
        editor.set_cursor(Index2::new(7, 0));

        editor.move_by_block(false);
        assert_eq!(editor.cursor(), Index2::new(5, 4), "this block's first line");
        editor.move_by_block(false);
        assert_eq!(editor.cursor(), Index2::new(0, 0), "then the previous block's");
        editor.move_by_block(false);
        assert_eq!(editor.cursor(), Index2::new(0, 0), "stays at the top");
    }

    #[test]
    fn from_a_blank_line_up_goes_to_the_block_above() {
        let mut editor = open_text(CODE);
        editor.set_cursor(Index2::new(4, 0));

        editor.move_by_block(false);

        assert_eq!(editor.cursor(), Index2::new(0, 0));
    }
}
