/// One conflict in the file being resolved, by row. SVN writes the
/// diff3 form, with the base section; plain two-way conflicts have none.
///
/// ```text
/// <<<<<<< .working          start
/// mine
/// ||||||| .merge-left.r1    base (optional)
/// what both started from
/// =======                   separator
/// theirs
/// >>>>>>> .merge-right.r2   end
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictRegion {
    pub start: usize,
    pub base: Option<usize>,
    pub separator: usize,
    pub end: usize,
}

/// What a row of the file is, as far as conflicts go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRole {
    Marker,
    Mine,
    Base,
    Theirs,
}

impl ConflictRegion {
    /// Every row of the region with its role, marker rows included.
    pub fn rows(&self) -> impl Iterator<Item = (usize, RowRole)> + '_ {
        let mine_end = self.base.unwrap_or(self.separator);
        (self.start..=self.end).map(move |row| {
            let role = if row == self.start || Some(row) == self.base || row == self.separator || row == self.end {
                RowRole::Marker
            } else if row < mine_end {
                RowRole::Mine
            } else if row < self.separator {
                RowRole::Base
            } else {
                RowRole::Theirs
            };
            (row, role)
        })
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Start,
    Base,
    Separator,
    End,
}


/// The complete conflicts in `text`, top to bottom. A region missing its
/// separator or end isn't one (a half-resolved conflict stops being
/// highlighted); a new `<<<<<<<` before the end starts over.
pub fn find_conflicts(text: &str) -> Vec<ConflictRegion> {
    let mut regions = Vec::new();
    let mut open: Option<(usize, Option<usize>, Option<usize>)> = None;
    for (row, line) in text.lines().enumerate() {
        match (marker(line), &mut open) {
            (Some(Marker::Start), _) => open = Some((row, None, None)),
            (Some(Marker::Base), Some((_, base @ None, None))) => *base = Some(row),
            (Some(Marker::Separator), Some((_, _, separator @ None))) => *separator = Some(row),
            (Some(Marker::End), Some((start, base, Some(separator)))) => {
                regions.push(ConflictRegion { start: *start, base: *base, separator: *separator, end: row });
                open = None;
            }
            _ => {}
        }
    }
    regions
}


/// Seven marker characters, then nothing or a space and a label; the
/// separator takes no label.
fn marker(line: &str) -> Option<Marker> {
    let line = line.trim_end();
    if line == "=======" {
        return Some(Marker::Separator);
    }
    let labeled = |run: &str| line.strip_prefix(run).is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
    if labeled("<<<<<<<") {
        Some(Marker::Start)
    } else if labeled("|||||||") {
        Some(Marker::Base)
    } else if labeled(">>>>>>>") {
        Some(Marker::End)
    } else {
        None
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_diff3_conflict_with_its_base_section() {
        let text = "a\n<<<<<<< .working\nmine\n||||||| .merge-left.r1\nbase\n=======\ntheirs\n>>>>>>> .merge-right.r2\nz\n";
        assert_eq!(find_conflicts(text), [ConflictRegion { start: 1, base: Some(3), separator: 5, end: 7 }]);
    }

    #[test]
    fn finds_two_way_conflicts_one_after_another() {
        let text = "<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\nx\n<<<<<<<\n=======\n>>>>>>>\n";
        assert_eq!(
            find_conflicts(text),
            [
                ConflictRegion { start: 0, base: None, separator: 2, end: 4 },
                ConflictRegion { start: 6, base: None, separator: 7, end: 8 },
            ]
        );
    }

    #[test]
    fn an_unfinished_conflict_is_not_one() {
        assert_eq!(find_conflicts("<<<<<<< .working\nmine\n=======\ntheirs\n"), []);
        assert_eq!(find_conflicts("<<<<<<< .working\nmine\n>>>>>>> .merge-right.r2\n"), [], "no separator");
    }

    #[test]
    fn a_new_start_marker_starts_over() {
        let text = "<<<<<<< stale\n<<<<<<< .working\nmine\n=======\ntheirs\n>>>>>>> .merge-right.r2\n";
        assert_eq!(find_conflicts(text), [ConflictRegion { start: 1, base: None, separator: 3, end: 5 }]);
    }

    #[test]
    fn longer_runs_and_glued_text_are_not_markers() {
        assert_eq!(marker("<<<<<<<< eight"), None);
        assert_eq!(marker("<<<<<<<x"), None);
        assert_eq!(marker("======== "), None);
        assert_eq!(marker("x <<<<<<<"), None);
        assert_eq!(marker("=======  "), Some(Marker::Separator), "trailing whitespace is fine");
    }

    #[test]
    fn rows_give_each_section_its_role() {
        let region = ConflictRegion { start: 1, base: Some(3), separator: 5, end: 7 };
        let roles: Vec<RowRole> = region.rows().map(|(_, role)| role).collect();
        use RowRole::*;
        assert_eq!(roles, [Marker, Mine, Marker, Base, Marker, Theirs, Marker]);

        let two_way = ConflictRegion { start: 0, base: None, separator: 2, end: 4 };
        let roles: Vec<RowRole> = two_way.rows().map(|(_, role)| role).collect();
        assert_eq!(roles, [Marker, Mine, Marker, Theirs, Marker]);
    }
}
