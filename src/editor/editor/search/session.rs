use edtui::{Index2, Lines, RowIndex};

/// One `Ctrl+F` search session -- our replacement for `edtui`'s
/// `SearchState`, whose case-insensitive compare allocates per character
/// and reran over the whole buffer on every edit (2.46s per typed
/// character on 100k lines; `SearchState` is `pub(crate)`, so it couldn't
/// be fed a faster compare). Same semantics:
///
/// - case-insensitive (Unicode lowercase), allocation-free
///   (`chars_eq_ignore_case`);
/// - a match never spans a line break;
/// - matches don't overlap, scanned greedily left to right;
/// - typing jumps to the first match at or after the search start,
///   wrapping.
///
/// Incremental: a match of a longer pattern is a match of its prefix, so
/// each added character only filters the previous candidates. Each prefix
/// length's candidates stay on a stack, so Backspace pops. Candidates
/// include overlapping positions (dropping them could lose a match as the
/// pattern grows); `matches` is recomputed from the top after each edit.
/// History: docs/history/editor-performance.md.
#[derive(Default)]
pub(super) struct SearchSession {
    pattern: Vec<char>,
    /// `candidates[i]`: every position (overlaps included) where
    /// `pattern[..=i]` matches, in buffer order.
    candidates: Vec<Vec<Index2>>,
    /// The greedy, non-overlapping subset of `candidates.last()` -- what
    /// `Enter`/`Shift+Enter` step through and what gets highlighted.
    matches: Vec<Index2>,
    selected: Option<usize>,
    /// Where the cursor was when the box opened -- where typing
    /// searches forward from, and where `Esc` returns to when nothing
    /// was found.
    start_cursor: Index2,
}


impl SearchSession {
    pub(super) fn new(start_cursor: Index2) -> Self {
        Self { start_cursor, ..Self::default() }
    }

    pub(super) fn start_cursor(&self) -> Index2 {
        self.start_cursor
    }

    #[cfg(test)]
    fn pattern(&self) -> String {
        self.pattern.iter().collect()
    }

    pub(super) fn pattern_len(&self) -> usize {
        self.pattern.len()
    }

    /// Appends `c` to the query and recomputes the matches -- see the
    /// struct's own doc comment for why this is a filter over the
    /// previous candidates, not a rescan.
    pub(super) fn push(&mut self, lines: &Lines, c: char) {
        self.pattern.push(c);
        let next = match self.candidates.last() {
            None => all_positions_of(lines, c),
            Some(previous) => {
                let offset = self.pattern.len() - 1;
                previous
                    .iter()
                    .copied()
                    .filter(|start| char_at(lines, start.row, start.col + offset).is_some_and(|found| chars_eq_ignore_case(found, c)))
                    .collect()
            }
        };
        self.candidates.push(next);
        self.recompute_matches();
    }

    /// Syncs with an arbitrarily edited query (the box is a real text field):
    /// keeps the candidate levels for the shared prefix and re-filters from
    /// there. Appending or Backspace at the end stay push/pop; an edit at the
    /// first character is one full scan.
    pub(super) fn set_pattern(&mut self, lines: &Lines, new_pattern: &str) {
        let new_pattern: Vec<char> = new_pattern.chars().collect();
        let common = self.pattern.iter().zip(&new_pattern).take_while(|(old, new)| old == new).count();
        self.pattern.truncate(common);
        self.candidates.truncate(common);
        for &c in &new_pattern[common..] {
            self.push(lines, c);
        }
        self.recompute_matches();
    }

    /// Selects the first match at or after `start_cursor` (wrapping to
    /// the very first match if there's none after it) and returns it --
    /// `edtui`'s own `SearchState::first`.
    pub(super) fn select_first_from_start(&mut self) -> Option<Index2> {
        let after_start = self.matches.iter().position(|&start| start >= self.start_cursor);
        self.selected = after_start.or_else(|| (!self.matches.is_empty()).then_some(0));
        self.selected_match()
    }

    /// `Enter`/`F3`: the first match after `caret`, wrapping -- measured from
    /// the caret, which can move while the box is open (VS Code). With focus
    /// in the box the caret is on the selected match, so this steps to the
    /// next one.
    pub(super) fn select_next_after(&mut self, caret: Index2) -> Option<Index2> {
        if self.matches.is_empty() {
            return None;
        }
        self.selected = Some(self.matches.iter().position(|&start| start > caret).unwrap_or(0));
        self.selected_match()
    }

    /// `Shift+Enter`/`Shift+F3`: the mirror of `select_next_after` -- the
    /// last match starting before `caret`, wrapping to the very last one.
    pub(super) fn select_previous_before(&mut self, caret: Index2) -> Option<Index2> {
        if self.matches.is_empty() {
            return None;
        }
        self.selected = Some(self.matches.iter().rposition(|&start| start < caret).unwrap_or(self.matches.len() - 1));
        self.selected_match()
    }

    /// Moves where "search from here" starts -- refocusing the box
    /// (`Ctrl+F` again) after the caret moved in the text.
    pub(super) fn set_start_cursor(&mut self, start_cursor: Index2) {
        self.start_cursor = start_cursor;
    }

    /// Recomputes every match against an edited buffer -- the box can
    /// stay open while the text itself is edited, and every stored
    /// candidate position is stale after that. A full rescan (every
    /// level's candidates depend on the buffer), keeping the selection
    /// on the same match if it still exists, or the first one after
    /// where it was.
    pub(super) fn rebuild(&mut self, lines: &Lines) {
        let previous = self.selected_match();
        let pattern = std::mem::take(&mut self.pattern);
        self.candidates.clear();
        for c in pattern {
            self.push(lines, c);
        }
        self.selected = previous.and_then(|previous| self.matches.iter().position(|&start| start >= previous));
    }

    /// The currently selected match's own start, if any.
    pub(super) fn selected_match(&self) -> Option<Index2> {
        self.selected.and_then(|index| self.matches.get(index).copied())
    }

    fn recompute_matches(&mut self) {
        self.matches.clear();
        self.selected = None;
        let Some(candidates) = self.candidates.last() else {
            return;
        };
        let len = self.pattern.len();
        let mut next_allowed: Option<Index2> = None;
        for &start in candidates {
            if next_allowed.is_some_and(|allowed| allowed.row == start.row && start.col < allowed.col) {
                continue;
            }
            self.matches.push(start);
            next_allowed = Some(Index2::new(start.row, start.col + len));
        }
    }
}


/// Every position in `lines` holding a character equal to `c`,
/// ignoring case -- the one full scan a session ever does, for its
/// first character.
fn all_positions_of(lines: &Lines, c: char) -> Vec<Index2> {
    let mut positions = Vec::new();
    for row_index in 0..lines.len() {
        let Some(row) = lines.get(RowIndex::new(row_index)) else {
            continue;
        };
        for (col, &found) in row.iter().enumerate() {
            if chars_eq_ignore_case(found, c) {
                positions.push(Index2::new(row_index, col));
            }
        }
    }
    positions
}

fn char_at(lines: &Lines, row: usize, col: usize) -> Option<char> {
    lines.get(RowIndex::new(row)).and_then(|row| row.get(col)).copied()
}

/// The same Unicode-lowercase comparison `edtui` makes, minus its two
/// `String` allocations: compares the two `to_lowercase()` iterators
/// directly, with an ASCII fast path for the overwhelmingly common case.
fn chars_eq_ignore_case(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn session_for(text: &str, query: &str) -> (Lines, SearchSession) {
        let lines = Lines::from(text);
        let mut session = SearchSession::new(Index2::new(0, 0));
        for c in query.chars() {
            session.push(&lines, c);
        }
        (lines, session)
    }

    /// The same fixture `edtui-jagged`'s own `test_match_indices` uses,
    /// so the replacement is checked against the original's own
    /// expected answers, not just against itself.
    #[test]
    fn matches_the_same_positions_edtui_did() {
        let (_, session) = session_for("aaBcaabc\n\naabc.", "abc");
        assert_eq!(session.matches, vec![Index2::new(0, 1), Index2::new(0, 5), Index2::new(2, 1)]);
    }

    #[test]
    fn is_case_insensitive_including_non_ascii() {
        let (_, session) = session_for("Привет мир\nПРИВЕТ", "привет");
        assert_eq!(session.matches, vec![Index2::new(0, 0), Index2::new(1, 0)]);
    }

    #[test]
    fn matches_never_overlap() {
        let (_, session) = session_for("aaaa", "aa");
        assert_eq!(session.matches, vec![Index2::new(0, 0), Index2::new(0, 2)], "not 0, 1, 2");
    }

    /// Why candidates keep overlapping positions even though matches
    /// don't: in "aaab", the non-overlapping "aa" matches are columns 0
    /// and 2 only, but "aab" itself matches starting at column 1 --
    /// dropping column 1 back at the "aa" stage would lose it.
    #[test]
    fn growing_the_pattern_still_finds_a_match_that_overlapped_a_shorter_one() {
        let (_, session) = session_for("aaab", "aab");
        assert_eq!(session.matches, vec![Index2::new(0, 1)]);
    }

    #[test]
    fn a_match_never_spans_a_line_break() {
        let (_, session) = session_for("ab\ncd", "bc");
        assert!(session.matches.is_empty());
    }

    #[test]
    fn shortening_the_pattern_restores_the_shorter_patterns_own_matches() {
        let (lines, mut session) = session_for("cat car cab", "cat");
        assert_eq!(session.matches, vec![Index2::new(0, 0)]);

        session.set_pattern(&lines, "ca");

        assert_eq!(session.pattern(), "ca");
        assert_eq!(session.matches, vec![Index2::new(0, 0), Index2::new(0, 4), Index2::new(0, 8)]);

        session.push(&lines, 'b');
        assert_eq!(session.matches, vec![Index2::new(0, 8)], "pushing again after shortening filters from the restored level");
    }

    /// An edit in the middle of the query (typing over a selection,
    /// deleting inside it) must land on exactly the matches a fresh
    /// search for the new text would find.
    #[test]
    fn set_pattern_after_a_mid_query_edit_matches_a_fresh_search() {
        let (lines, mut session) = session_for("cart cat cab cot", "cart");

        session.set_pattern(&lines, "cot");

        let (_, fresh) = session_for("cart cat cab cot", "cot");
        assert_eq!(session.pattern(), "cot");
        assert_eq!(session.matches, fresh.matches);
    }

    #[test]
    fn set_pattern_to_empty_clears_everything() {
        let (lines, mut session) = session_for("abc", "ab");
        session.set_pattern(&lines, "");
        assert!(session.matches.is_empty());
        assert_eq!(session.pattern_len(), 0);
    }

    #[test]
    fn select_first_prefers_a_match_at_or_after_the_start_then_wraps() {
        let lines = Lines::from("dog cat dog");
        let mut session = SearchSession::new(Index2::new(0, 5));
        for c in "dog".chars() {
            session.push(&lines, c);
        }
        assert_eq!(session.select_first_from_start(), Some(Index2::new(0, 8)));

        let mut session = SearchSession::new(Index2::new(0, 9));
        for c in "cat".chars() {
            session.push(&lines, c);
        }
        assert_eq!(session.select_first_from_start(), Some(Index2::new(0, 4)), "nothing after the start -- wraps to the first");
    }

    #[test]
    fn next_and_previous_are_measured_from_the_caret_and_wrap() {
        let (_, mut session) = session_for("a a a", "a");
        assert_eq!(session.select_next_after(Index2::new(0, 0)), Some(Index2::new(0, 2)));
        assert_eq!(session.select_next_after(Index2::new(0, 3)), Some(Index2::new(0, 4)), "from wherever the caret moved to");
        assert_eq!(session.select_next_after(Index2::new(0, 4)), Some(Index2::new(0, 0)), "wraps past the last");
        assert_eq!(session.select_previous_before(Index2::new(0, 3)), Some(Index2::new(0, 2)));
        assert_eq!(session.select_previous_before(Index2::new(0, 0)), Some(Index2::new(0, 4)), "wraps past the first");
    }

    #[test]
    fn next_with_no_matches_selects_nothing() {
        let (_, mut session) = session_for("abc", "x");
        assert_eq!(session.select_next_after(Index2::new(0, 0)), None);
    }

    /// The box stays open while the text is edited, so stored positions
    /// go stale -- a rebuild finds the matches in the edited text and
    /// keeps the selection on the same match.
    #[test]
    fn rebuild_after_an_edit_finds_the_moved_matches_and_keeps_the_selection() {
        let (_, mut session) = session_for("cat cat", "cat");
        session.select_next_after(Index2::new(0, 0)); // selects the second "cat", (0, 4)

        let edited = Lines::from("xx cat cat");
        session.rebuild(&edited);

        assert_eq!(session.matches, vec![Index2::new(0, 3), Index2::new(0, 7)]);
        assert_eq!(session.selected_match(), Some(Index2::new(0, 7)), "the first match at or after where the selected one was");
    }
}
