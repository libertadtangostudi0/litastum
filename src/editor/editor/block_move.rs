use edtui::actions::{Chainable, Execute, SwitchMode};
use edtui::{EditorMode, Index2, RowIndex};

use super::word_select_touch::WordSelectTouch;
use super::Editor;

/// Data formats, which have nesting rather than blank lines between
/// blocks: `Ctrl+Up`/`Down` go by levels there (`next_at_level`).
const NESTED_DATA_EXTENSIONS: &[&str] = &["json", "jsonc", "json5", "yaml", "yml", "xml", "xaml", "svg", "plist", "csproj", "vcxproj", "props", "targets", "html", "htm"];

/// A tab's width when comparing indents.
const TAB_INDENT: usize = 4;

impl Editor {
    /// `Ctrl+Down`/`Ctrl+Up` (`Standard`): the caret jumps between blocks
    /// of code -- runs of non-blank lines. Down: the first line of the
    /// next block (the last line if there is none). Up: the first line of
    /// the current block, or of the previous one from there or from a
    /// blank line. In a data format (JSON, YAML, XML) -- one block with no
    /// blank lines, jumped through at once (reported) -- by levels
    /// instead (`next_at_level`). The caret lands on the line's first
    /// non-blank character; a selection is dropped, as by a plain arrow.
    /// Scans only the rows it passes.
    pub fn move_by_block(&mut self, forward: bool) {
        let row = self.state.cursor.row;
        let target = match (self.moves_by_levels(), forward) {
            (true, true) => self.next_at_level(row),
            (true, false) => self.previous_at_level(row),
            (false, true) => self.next_block_start(row),
            (false, false) => self.block_start_above(row),
        };
        if self.state.selection.is_some() {
            SwitchMode(EditorMode::Normal).chain(SwitchMode(EditorMode::Insert)).execute(&mut self.state);
            self.vertical_shift_anchor = None;
            self.word_select_true_anchor = None;
            self.word_select_touch = WordSelectTouch::Untouched;
        }
        self.state.cursor = Index2::new(target, self.first_non_blank_column(target));
    }

    fn moves_by_levels(&self) -> bool {
        let extension = self.path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
        extension.is_some_and(|extension| NESTED_DATA_EXTENSIONS.contains(&extension.as_str()))
    }

    /// Down by levels, as in a tree: the next line as deep as this one or
    /// shallower -- the next key or element, its nested lines passed over;
    /// after the last one at its level, the next one up. Blank lines and
    /// lines that only close (`}`, `],`, `</tag>`) aren't stops. The last
    /// line if there's none.
    fn next_at_level(&self, row: usize) -> usize {
        let level = self.stop_indent(row).unwrap_or(usize::MAX);
        let last = self.line_count().saturating_sub(1);
        (row + 1..self.line_count()).find(|&next| self.stop_indent(next).is_some_and(|indent| indent <= level)).unwrap_or(last)
    }

    /// Up by levels: the previous line as deep as this one or shallower --
    /// the previous key or element, or from the first one its parent.
    fn previous_at_level(&self, row: usize) -> usize {
        let level = self.stop_indent(row).or_else(|| self.indent(row)).unwrap_or(usize::MAX);
        (0..row).rev().find(|&up| self.stop_indent(up).is_some_and(|indent| indent <= level)).unwrap_or(0)
    }

    /// `row`'s indent, if it's a stop for a move by levels: not blank, not
    /// only closing something.
    fn stop_indent(&self, row: usize) -> Option<usize> {
        let chars = self.state.lines.get(RowIndex::new(row))?;
        let text: String = chars.iter().collect();
        let trimmed = text.trim_start();
        let closes = trimmed.starts_with(['}', ']', ')']) || trimmed.starts_with("</");
        if trimmed.is_empty() || closes {
            return None;
        }
        self.indent(row)
    }

    /// `row`'s indent in columns, a tab counting `TAB_INDENT`.
    fn indent(&self, row: usize) -> Option<usize> {
        let chars = self.state.lines.get(RowIndex::new(row))?;
        Some(chars.iter().take_while(|c| c.is_whitespace()).map(|&c| if c == '\t' { TAB_INDENT } else { 1 }).sum())
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
        open_named("code.rs", text)
    }

    fn open_named(name: &str, text: &str) -> Editor {
        let path = unique_scratch_dir("block-move").join(name);
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


    /// Rows: 0 `{`, 1 `"a": {`, 2 `"x": 1,`, 3 `"y": [`, 4 `1`, 5 `],`,
    /// 6 `"z": 2`, 7 `},`, 8 `"b": 3,`, 9 `"c": {`, 10 `"d": 4`, 11 `}`,
    /// 12 `}`.
    const JSON: &str = "{\n  \"a\": {\n    \"x\": 1,\n    \"y\": [\n      1\n    ],\n    \"z\": 2\n  },\n  \"b\": 3,\n  \"c\": {\n    \"d\": 4\n  }\n}\n";

    fn rows_down(editor: &mut Editor, from: usize, times: usize) -> Vec<usize> {
        editor.set_cursor(Index2::new(from, 0));
        (0..times)
            .map(|_| {
                editor.move_by_block(true);
                editor.cursor().row
            })
            .collect()
    }

    /// Reported: a JSON file, with no blank lines, was one block -- the
    /// caret jumped from the top to the end. By levels: the next key at
    /// this level, its nested lines passed over.
    #[test]
    fn json_goes_down_by_levels() {
        let mut editor = open_named("data.json", JSON);
        let last = editor.line_count() - 1;
        assert_eq!(rows_down(&mut editor, 1, 3), [8, 9, last], "a -> b -> c -> the end");
        assert_eq!(rows_down(&mut editor, 2, 3), [3, 6, 8], "x -> y -> z (y's array passed over) -> up to b");
        assert_eq!(editor.cursor().col, 2, "on the key's first character");
    }

    #[test]
    fn json_goes_up_by_levels() {
        let mut editor = open_named("data.json", JSON);
        editor.set_cursor(Index2::new(9, 0));
        let mut rows = Vec::new();
        for _ in 0..3 {
            editor.move_by_block(false);
            rows.push(editor.cursor().row);
        }
        assert_eq!(rows, [8, 1, 0], "c -> b -> a (its nested lines passed over) -> the opening brace");

        editor.set_cursor(Index2::new(6, 0));
        editor.move_by_block(false);
        assert_eq!(editor.cursor().row, 3, "z -> y");
        editor.set_cursor(Index2::new(2, 0));
        editor.move_by_block(false);
        assert_eq!(editor.cursor().row, 1, "the first key -> its parent");
    }

    #[test]
    fn xml_goes_by_levels_past_closing_tags() {
        let mut editor = open_named("a.XML", "<root>\n  <a>\n    <x/>\n  </a>\n  <b/>\n</root>\n");
        assert_eq!(rows_down(&mut editor, 1, 1), [4], "<a> -> <b/>; </a> isn't a stop");
    }

    /// Code keeps its blocks between blank lines.
    #[test]
    fn code_still_goes_by_blank_line_blocks() {
        let mut editor = open_text("fn a() {\n    one();\n    two();\n}\n");
        let last = editor.line_count() - 1;
        assert_eq!(rows_down(&mut editor, 0, 1), [last], "one block: to the end, as before");
    }
}
