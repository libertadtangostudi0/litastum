use edtui::{Highlight, Index2};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    Frame,
};

use crate::compare::{map_real_row, DiffLineKind, DiffLines, LineEndingDisplay};
use crate::conflict::{ConflictRegion, ConflictState, Pane, RowRole};
use crate::editor::Editor;
use crate::theming::{blend_over_bg, Theme};

use super::compare::{draw_compare_panes, row_highlights, ChangeColors};
use super::path_edit::draw_path_field;

/// The conflict resolver, after Araxis Merge: `.working` | the file being
/// resolved (twice as wide) | `.merge-right` on top, about three fifths
/// of the height; the incoming change's Compare below. No hint row: the
/// height goes to the editors (requested). The side panes are diffed against the result every frame, and the top
/// panes follow the focused one, as in Compare.
pub(super) fn draw_conflict(frame: &mut Frame, area: Rect, state: &mut ConflictState, theme: &Theme, line_ending_display: LineEndingDisplay) -> Option<Position> {
    let [top, bottom] = Layout::vertical([Constraint::Fill(3), Constraint::Fill(2)]).areas(area);
    let [working_area, result_area, theirs_area] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(2), Constraint::Fill(1)]).areas(top);

    let ConflictState { working, result, theirs, working_diff, theirs_diff, conflicts, highlighted, focus, .. } = state;
    let ((working_side, result_vs_working), _) = working_diff.get(working, result);
    let ((result_vs_theirs, theirs_side), _) = theirs_diff.get(result, theirs);
    let regions = conflicts.get(result);

    // Highlights are rebuilt only when a text or the theme changed: moving
    // the caret used to rebuild them, and both diffs, every frame.
    let colors = ConflictColors::new(theme);
    let key = (working.revision(), result.revision(), theirs.revision(), colors.key());
    if *highlighted != Some(key) {
        *highlighted = Some(key);
        let working_highlights = row_highlights(working_side, result_vs_working, true, working, result, ChangeColors::from(theme.success, colors.mine_bg, theme), theme);
        let theirs_highlights = row_highlights(theirs_side, result_vs_theirs, false, theirs, result, ChangeColors::from(theme.accent, colors.theirs_bg, theme), theme);
        working.set_extra_highlights(working_highlights);
        theirs.set_extra_highlights(theirs_highlights);
        let highlights = result_highlights(regions, result_vs_working, result_vs_theirs, result, &colors, theme);
        result.set_extra_highlights(highlights);
    }
    let diffs = TopDiffs { working: working_side, result_vs_working, result_vs_theirs, theirs: theirs_side };
    let focus = *focus;
    let areas = [(Pane::Working, working_area), (Pane::Result, result_area), (Pane::Theirs, theirs_area)];

    // The focused pane first: drawing scrolls it to its caret, and the
    // others follow in the same frame -- aligned before it, they lagged a
    // key behind (requested).
    let mut cursor = None;
    let mut focused_title = None;
    if let Some(&(pane, pane_area)) = areas.iter().find(|(pane, _)| *pane == focus) {
        let editor = top_editor(pane, working, result, theirs);
        frame.render_widget(editor.view(theme, pane_area), pane_area);
        cursor = editor.cursor_screen_position();
        focused_title = Some(editor.title_area());
    }
    align_top_panes(focus, working, result, theirs, &diffs);
    for (pane, pane_area) in areas.into_iter().filter(|(pane, _)| *pane != focus) {
        let editor = top_editor(pane, working, result, theirs);
        frame.render_widget(editor.view(theme, pane_area), pane_area);
    }
    let incoming_cursor = draw_compare_panes(frame, bottom, &mut state.incoming, theme, line_ending_display);
    if focus == Pane::Incoming {
        cursor = incoming_cursor;
    }
    // Last, so a `Tab` list reaching down isn't drawn over by the bottom
    // Compare.
    if let (Some(edit), Some(title)) = (&state.path_edit, focused_title) {
        cursor = draw_path_field(frame, title, edit, theme);
    }
    cursor
}


/// The top pane `pane`'s editor (`pane` is never `Incoming`).
fn top_editor<'a>(pane: Pane, working: &'a mut Editor, result: &'a mut Editor, theirs: &'a mut Editor) -> &'a mut Editor {
    match pane {
        Pane::Working => working,
        Pane::Theirs => theirs,
        Pane::Result | Pane::Incoming => result,
    }
}


/// The top panes' two diffs, each side: `.working` against the result,
/// the result against `.merge-right`.
struct TopDiffs<'a> {
    working: &'a DiffLines,
    result_vs_working: &'a DiffLines,
    result_vs_theirs: &'a DiffLines,
    theirs: &'a DiffLines,
}


/// Scrolls the unfocused top panes level with the focused one, mapping
/// its top row through the diffs (`map_real_row`); the side panes only
/// relate to each other through the result. Untouched while the bottom
/// Compare has focus.
fn align_top_panes(focus: Pane, working: &mut Editor, result: &mut Editor, theirs: &mut Editor, diffs: &TopDiffs) {
    let result_top = match focus {
        Pane::Working => {
            let top = map_real_row(&diffs.working.source_index, &diffs.result_vs_working.source_index, working.viewport_top_row());
            result.set_viewport_top_row(top);
            top
        }
        Pane::Theirs => {
            let top = map_real_row(&diffs.theirs.source_index, &diffs.result_vs_theirs.source_index, theirs.viewport_top_row());
            result.set_viewport_top_row(top);
            top
        }
        Pane::Result => result.viewport_top_row(),
        Pane::Incoming => return,
    };
    if focus != Pane::Working {
        working.set_viewport_top_row(map_real_row(&diffs.result_vs_working.source_index, &diffs.working.source_index, result_top));
    }
    if focus != Pane::Theirs {
        theirs.set_viewport_top_row(map_real_row(&diffs.result_vs_theirs.source_index, &diffs.theirs.source_index, result_top));
    }
}


/// The resolver's colors, after VS Code's merge editor: mine green, theirs
/// blue, the base gray, conflict markers bold on a `warning` wash.
/// Blended over `bg` like Compare's diff colors, at render time (a
/// `Theme` has no conflict fields).
struct ConflictColors {
    mine_bg: Color,
    theirs_bg: Color,
    base_bg: Color,
    marker_bg: Color,
}

impl ConflictColors {
    fn key(&self) -> [Color; 4] {
        [self.mine_bg, self.theirs_bg, self.base_bg, self.marker_bg]
    }

    fn new(theme: &Theme) -> Self {
        Self {
            mine_bg: theme.diff_added_bg,
            theirs_bg: blend_over_bg(theme.accent, theme.bg, 0.3),
            base_bg: blend_over_bg(theme.text_dim, theme.bg, 0.3),
            marker_bg: blend_over_bg(theme.warning, theme.bg, 0.3),
        }
    }
}


/// The result's backgrounds. Inside a conflict, by section; elsewhere by
/// where a line came from: differing only from `.working` means theirs
/// (blue), only from `.merge-right` means mine (green), from both -- an
/// edit of its own -- the marker wash without bold.
fn result_highlights(conflicts: &[ConflictRegion], result_vs_working: &DiffLines, result_vs_theirs: &DiffLines, result: &Editor, colors: &ConflictColors, theme: &Theme) -> Vec<Highlight> {
    let line_count = result.line_count();
    let mut differs_from_working = vec![false; line_count];
    let mut differs_from_theirs = vec![false; line_count];
    mark_changed_rows(result_vs_working, DiffLineKind::Added, &mut differs_from_working);
    mark_changed_rows(result_vs_theirs, DiffLineKind::Removed, &mut differs_from_theirs);
    let mut in_conflict = vec![false; line_count];

    let flat = |bg: Color| Style::default().fg(theme.text).bg(bg);
    let line_highlight = |row: usize, style: Style| {
        let end_col = result.line_len(row).saturating_sub(1);
        Highlight::new(Index2::new(row, 0), Index2::new(row, end_col), style)
    };

    let mut highlights = Vec::new();
    for (row, role) in conflicts.iter().flat_map(ConflictRegion::rows) {
        if row >= line_count {
            continue;
        }
        in_conflict[row] = true;
        let style = match role {
            RowRole::Marker => flat(colors.marker_bg).add_modifier(Modifier::BOLD),
            RowRole::Mine => flat(colors.mine_bg),
            RowRole::Base => flat(colors.base_bg),
            RowRole::Theirs => flat(colors.theirs_bg),
        };
        highlights.push(line_highlight(row, style));
    }
    for row in (0..line_count).filter(|&row| !in_conflict[row]) {
        let bg = match (differs_from_working[row], differs_from_theirs[row]) {
            (true, false) => colors.theirs_bg,
            (false, true) => colors.mine_bg,
            (true, true) => colors.marker_bg,
            (false, false) => continue,
        };
        highlights.push(line_highlight(row, flat(bg)));
    }
    highlights
}


fn mark_changed_rows(diff: &DiffLines, changed: DiffLineKind, rows: &mut [bool]) {
    for (kind, real_row) in diff.kinds.iter().zip(&diff.source_index) {
        if let (true, Some(real_row)) = (*kind == changed, *real_row) {
            if let Some(row) = rows.get_mut(real_row) {
                *row = true;
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::compare::Side;
    use crate::conflict::state_tests::open_conflict;

    fn render(state: &mut ConflictState) -> Option<Position> {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let mut cursor = None;
        terminal.draw(|frame| cursor = draw_conflict(frame, frame.area(), state, &Theme::dark(), LineEndingDisplay::Hidden)).unwrap();
        cursor
    }

    fn left_click(column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
    }

    /// 80x20: the top is rows 0-11 split 20 | 40 | 20 columns, the bottom
    /// rows 12-19 split 40 | 40.
    #[test]
    fn the_panes_follow_the_layout_proportions() {
        let mut state = open_conflict();
        render(&mut state);

        assert!(state.working.contains_screen_position(1, 1));
        assert!(state.result.contains_screen_position(21, 1));
        assert!(state.result.contains_screen_position(58, 11));
        assert!(state.theirs.contains_screen_position(61, 1));
        assert!(state.incoming.left.contains_screen_position(1, 12));
        assert!(state.incoming.right.contains_screen_position(41, 19));
    }

    /// The fixture's conflict: rows 1-5 of the result, which starts at
    /// x = 21 in the 40-column middle pane (border + 2-column gutter
    /// before the text at x = 24).
    #[test]
    fn each_part_of_a_conflict_gets_its_own_background() {
        let mut state = open_conflict();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| { draw_conflict(frame, frame.area(), &mut state, &theme, LineEndingDisplay::Hidden); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let bg_at = |row: u16| buffer[(24, row + 1)].bg;

        let marker = ConflictColors::new(&theme).marker_bg;
        assert_ne!(bg_at(0), marker, "\"one\" is outside the conflict");
        assert_eq!(bg_at(1), marker, "<<<<<<<");
        assert!(buffer[(24, 2)].modifier.contains(Modifier::BOLD));
        assert_eq!(bg_at(2), theme.diff_added_bg, "mine");
        assert_eq!(bg_at(3), marker, "=======");
        assert_eq!(bg_at(4), blend_over_bg(theme.accent, theme.bg, 0.3), "theirs");
        assert_eq!(bg_at(5), marker, ">>>>>>>");
    }

    fn open_with(working: &str, result: &str, theirs: &str) -> ConflictState {
        let dir = crate::test_support::unique_scratch_dir("ui-conflict");
        let files = crate::conflict::detect(&crate::conflict::state_tests::write_conflict_files(&dir)).unwrap();
        std::fs::write(&files.working, working).unwrap();
        std::fs::write(&files.result, result).unwrap();
        std::fs::write(&files.theirs, theirs).unwrap();
        ConflictState::open(files, None, crate::editor::EditorKeymapMode::Standard).unwrap()
    }

    /// Outside a conflict: "w" is mine (only `.merge-right` lacks it),
    /// "t" is theirs (only `.working` lacks it). The side panes color what
    /// they have that the result doesn't: `.working`'s "c", `.merge-right`'s "z".
    #[test]
    fn the_side_panes_and_the_result_show_where_each_change_came_from() {
        let mut state = open_with("a\nw\nb\nc\n", "a\nw\nb\nt\n", "a\nb\nt\nz\n");
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| { draw_conflict(frame, frame.area(), &mut state, &theme, LineEndingDisplay::Hidden); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let colors = ConflictColors::new(&theme);

        // Text starts at x = 3 in the side panes (1-digit gutter), x = 23 in the middle one.
        assert_eq!(buffer[(23, 2)].bg, colors.mine_bg, "result \"w\"");
        assert_eq!(buffer[(23, 4)].bg, colors.theirs_bg, "result \"t\"");
        assert_ne!(buffer[(23, 1)].bg, colors.mine_bg, "\"a\" is everywhere");
        assert_eq!(buffer[(3, 4)].bg, colors.mine_bg, ".working's \"c\" isn't in the result");
        assert_eq!(buffer[(63, 4)].bg, colors.theirs_bg, ".merge-right's \"z\"");
        assert_ne!(buffer[(63, 3)].bg, colors.theirs_bg, ".merge-right's \"t\" is in the result");
    }

    /// The result has three extra lines at the top; scrolled down, the
    /// side panes stay level with it, three rows behind.
    #[test]
    fn the_side_panes_scroll_with_the_focused_result() {
        let side: String = (0..60).map(|i| format!("l{i}\n")).collect();
        let result = format!("x0\nx1\nx2\n{side}");
        let mut state = open_with(&side, &result, &side);
        // A first frame gives the panes their size, as on opening.
        render(&mut state);
        state.result.set_cursor(Index2::new(40, 0));
        // One frame: the result scrolls to its caret while it's drawn, and
        // the side panes follow it in that same frame (requested: they
        // lagged a key behind).
        render(&mut state);

        let top = state.result.viewport_top_row();
        assert!(top > 3, "the result scrolled");
        assert_eq!(state.working.viewport_top_row(), top - 3);
        assert_eq!(state.theirs.viewport_top_row(), top - 3);
    }

    #[test]
    fn a_click_on_a_top_title_edits_that_panes_path() {
        let mut state = open_conflict();
        render(&mut state);

        state.mouse(left_click(65, 0));
        assert_eq!(state.focus, Pane::Theirs);
        assert!(state.path_edit.is_some());
        assert_eq!(render(&mut state).map(|position| position.y), Some(0), "the caret is in the field");

        state.mouse(left_click(30, 3));
        assert!(state.path_edit.is_none());
        assert_eq!(state.focus, Pane::Result);
    }

    #[test]
    fn a_click_focuses_the_pane_under_the_pointer() {
        let mut state = open_conflict();
        render(&mut state);

        state.mouse(left_click(65, 2));
        assert_eq!(state.focus, Pane::Theirs);

        state.mouse(left_click(50, 14));
        assert_eq!((state.focus, state.incoming.focus), (Pane::Incoming, Side::Right));
        assert!(render(&mut state).is_some(), "the focused bottom pane owns the terminal cursor");

        state.mouse(left_click(5, 2));
        assert_eq!(state.focus, Pane::Working);
    }

    /// Reported: the wheel sometimes did nothing. Over an unfocused top
    /// pane it scrolled that pane, which the next frame lined up with the
    /// focused one again; it now scrolls the focused pane, the others
    /// following.
    #[test]
    fn the_wheel_over_an_unfocused_top_pane_scrolls_the_focused_one() {
        let dir = crate::test_support::unique_scratch_dir("conflict-wheel");
        let body: String = (0..100).map(|n| format!("line {n}
")).collect();
        let result = format!("{body}<<<<<<< .working
mine
=======
theirs
>>>>>>> .merge-right.r2
");
        for (name, content) in [("a.txt", result.as_str()), ("a.txt.merge-left.r1", body.as_str()), ("a.txt.merge-right.r2", body.as_str()), ("a.txt.working", body.as_str())] {
            std::fs::write(dir.join(name), content).unwrap();
        }
        let files = crate::conflict::detect(&["a.txt", "a.txt.merge-left.r1", "a.txt.merge-right.r2", "a.txt.working"].map(|name| dir.join(name))).unwrap();
        let mut state = ConflictState::open(files, None, crate::editor::EditorKeymapMode::Standard).unwrap();
        render(&mut state);
        let wheel = MouseEvent { kind: MouseEventKind::ScrollDown, column: 5, row: 5, modifiers: KeyModifiers::NONE };

        for _ in 0..4 {
            state.mouse(wheel);
            render(&mut state);
        }

        assert_eq!(state.focus, crate::conflict::Pane::Result, "the wheel doesn't move the focus");
        assert_eq!(state.result.viewport_top_row(), 12, "the focused pane scrolled four steps");
        assert!(state.working.viewport_top_row() > 0, "and the pane under the pointer followed it");
    }
}
