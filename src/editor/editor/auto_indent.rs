//! Enter keeps the line's indentation, and Backspace in an indentation of
//! spaces takes a whole level (requested: with Enter starting every line
//! at column 0, indentation was retyped by hand and came out uneven).
//! History: docs/history/editor-saving.md.

use edtui::{Index2, RowIndex};

use super::Editor;

/// Extensions whose blocks open with a `:` at a line's end.
const COLON_BLOCK_EXTENSIONS: &[&str] = &["py", "pyw", "yaml", "yml"];


impl Editor {
    /// Enter (`Standard`): splits the line at the caret, the new line
    /// taking the old one's indentation as it is -- tabs or spaces -- and
    /// a level more after an opening bracket (or a `:` in Python and
    /// YAML). Between a bracket and its closer the closer goes a line
    /// further down, at the old indentation. A line left with only its
    /// indentation is emptied: no trailing whitespace. Over a selection,
    /// replaces it. Undo was recorded by the caller.
    pub(super) fn enter(&mut self) {
        self.row_edit = None;
        self.delete_selection();
        if self.state.lines.is_empty() {
            self.state.lines.push(Vec::new());
        }
        let Index2 { row, col } = self.state.cursor;
        let unit = self.indent.tab_text(0);
        let colon_opens = self.colon_opens_a_block();
        let Some(line) = self.state.lines.get_mut(RowIndex::new(row)) else {
            return;
        };
        let col = col.min(line.len());
        let mut after = line.split_off(col);
        let indent: Vec<char> = line.iter().take_while(|&&c| is_indent(c)).copied().collect();

        if line.len() == indent.len() {
            // At the indentation or before it: the line moves down whole.
            line.clear();
            let mut moved = indent;
            moved.append(&mut after);
            self.state.lines.insert(RowIndex::new(row + 1), moved);
            self.state.cursor = Index2::new(row + 1, col);
            self.buffer_changed();
            return;
        }

        let opener = line.iter().rev().find(|&&c| !is_indent(c)).copied().filter(|&c| opens_a_block(c, colon_opens));
        let after: Vec<char> = after.into_iter().skip_while(|&c| is_indent(c)).collect();
        let mut caret_line = indent.clone();
        if opener.is_some() {
            caret_line.extend(&unit);
        }
        let caret_col = caret_line.len();
        if opener.and_then(closer_of).is_some_and(|closer| after.first() == Some(&closer)) {
            let mut closing = indent;
            closing.extend(after);
            self.state.lines.insert(RowIndex::new(row + 1), caret_line);
            self.state.lines.insert(RowIndex::new(row + 2), closing);
        } else {
            caret_line.extend(after);
            self.state.lines.insert(RowIndex::new(row + 1), caret_line);
        }
        self.state.cursor = Index2::new(row + 1, caret_col);
        self.buffer_changed();
    }


    /// Backspace with only spaces before the caret: back to the previous
    /// indent stop, a whole level -- one Tab's spaces take one Backspace.
    /// `false` (left to the usual Backspace) anywhere else, or for one
    /// space. Undo was recorded by the caller.
    pub(super) fn backspace_in_indent(&mut self) -> bool {
        if self.state.selection.is_some() {
            return false;
        }
        let Index2 { row, col } = self.state.cursor;
        let width = self.indent.width();
        let Some(line) = self.state.lines.get_mut(RowIndex::new(row)) else {
            return false;
        };
        if col == 0 || col > line.len() || !line[..col].iter().all(|&c| c == ' ') {
            return false;
        }
        let back = match col % width {
            0 => width,
            partial => partial,
        };
        if back < 2 {
            return false;
        }
        line.drain(col - back..col);
        self.state.cursor.col = col - back;
        self.row_edit = None;
        self.row_changed(row);
        true
    }


    fn colon_opens_a_block(&self) -> bool {
        let extension = self.path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
        extension.is_some_and(|extension| COLON_BLOCK_EXTENSIONS.contains(&extension.as_str()))
    }
}


fn is_indent(c: char) -> bool {
    c == ' ' || c == '\t'
}


fn opens_a_block(c: char, colon_opens: bool) -> bool {
    matches!(c, '{' | '(' | '[') || (colon_opens && c == ':')
}


fn closer_of(opener: char) -> Option<char> {
    match opener {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    }
}
