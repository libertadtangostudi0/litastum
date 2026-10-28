use edtui::{Index2, RowIndex};

use super::word_select_touch::WordSelectTouch;
use super::Editor;

impl Editor {
    /// Tab (`Standard`): over a selection of several lines, indents each
    /// one by a level, the selection kept; otherwise types the file's
    /// indent at the caret (`text_format::Indent::tab_text`), over a
    /// selection on one line. Undo was recorded by the caller.
    pub(super) fn tab(&mut self) {
        self.row_edit = None;
        if let Some((first, last)) = self.selected_rows().filter(|_| self.selection_takes_a_line_break()) {
            for row in first..=last {
                self.shift_row(row, true);
            }
            self.buffer_changed();
            return;
        }
        let replaced = self.delete_selection();
        if self.state.lines.is_empty() {
            self.state.lines.push(Vec::new());
        }
        let Index2 { row, col } = self.state.cursor;
        let indent = self.indent;
        let Some(line) = self.state.lines.get_mut(RowIndex::new(row)) else {
            return;
        };
        let col = col.min(line.len());
        let text = indent.tab_text(indent.column(&line[..col]));
        let typed = text.len();
        line.splice(col..col, text);
        self.state.cursor = Index2::new(row, col + typed);
        if replaced {
            self.buffer_changed();
        } else {
            self.row_changed(row);
        }
    }


    /// Shift+Tab (`Standard`): one level less on the caret's line, or on
    /// every selected line, the selection kept. Undo was recorded by the
    /// caller.
    pub(super) fn outdent(&mut self) {
        self.row_edit = None;
        match self.selected_rows() {
            Some((first, last)) => {
                for row in first..=last {
                    self.shift_row(row, false);
                }
                self.buffer_changed();
            }
            None => {
                let row = self.state.cursor.row;
                self.shift_row(row, false);
                self.row_changed(row);
            }
        }
    }


    /// The first and last rows the selection touches.
    fn selected_rows(&self) -> Option<(usize, usize)> {
        let selection = self.state.selection.as_ref()?;
        Some((selection.start.row.min(selection.end.row), selection.start.row.max(selection.end.row)))
    }


    /// Whether the selection goes over a line break -- several lines, or a
    /// whole line taken by `Shift+Down` from its start, which ends on its
    /// break (`(row, len)`): Tab indents those lines rather than replacing
    /// them.
    fn selection_takes_a_line_break(&self) -> bool {
        let Some(selection) = self.state.selection.as_ref() else {
            return false;
        };
        let (start, end) = if (selection.start.row, selection.start.col) <= (selection.end.row, selection.end.col) {
            (selection.start, selection.end)
        } else {
            (selection.end, selection.start)
        };
        start.row < end.row || end.col >= self.line_len(end.row)
    }


    /// One level more (`deeper`) or less at `row`'s start, the caret and
    /// the selection moving with the text. An empty row isn't indented.
    fn shift_row(&mut self, row: usize, deeper: bool) {
        let indent = self.indent;
        let Some(line) = self.state.lines.get_mut(RowIndex::new(row)) else {
            return;
        };
        if deeper {
            if line.is_empty() {
                return;
            }
            let unit = indent.tab_text(0);
            let added = unit.len();
            line.splice(0..0, unit);
            self.move_columns_on(row, |col| col + added);
        } else {
            let removed = indent.outdent_len(line);
            if removed == 0 {
                return;
            }
            line.drain(..removed);
            self.move_columns_on(row, |col| col.saturating_sub(removed));
        }
    }


    /// Moves every position on `row` -- the caret, the selection's ends --
    /// by `shift`. The word and vertical selection anchors are dropped:
    /// they'd point at the old columns.
    fn move_columns_on(&mut self, row: usize, shift: impl Fn(usize) -> usize) {
        if self.state.cursor.row == row {
            self.state.cursor.col = shift(self.state.cursor.col);
        }
        if let Some(selection) = &mut self.state.selection {
            for end in [&mut selection.start, &mut selection.end] {
                if end.row == row {
                    end.col = shift(end.col);
                }
            }
        }
        self.vertical_shift_anchor = None;
        self.word_select_true_anchor = None;
        self.word_select_touch = WordSelectTouch::Untouched;
    }
}
