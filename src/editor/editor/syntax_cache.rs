//! Syntax highlighting that carries `syntect`'s state from line to line.
//! `edtui`'s own highlighter starts every line afresh, so a block comment,
//! a multi-line string or a fenced code block lost its color after its
//! first line, and the words in it were colored as code (reported). Here
//! the buffer is parsed from the top, the state kept every
//! `CHECKPOINT_ROWS` rows, and only the rows on screen are turned into
//! `Highlight`s for `edtui`'s plain render path -- kept per row until an
//! edit above them, since `syntect` takes ~0.1 ms a row.
//! History: docs/history/editor-rendering.md.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use edtui::syntect::highlighting::{FontStyle, HighlightState, Highlighter, RangedHighlightIterator, Theme as SynTheme};
use edtui::syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
use edtui::{Highlight, Index2, Lines, RowIndex};
use ratatui::style::{Color, Modifier, Style};

use super::changes::row_hash;

/// Rows between two kept states: getting from one to the first row on
/// screen parses at most this many rows it doesn't show.
const CHECKPOINT_ROWS: usize = 64;

/// How long a frame may parse to catch up with a view far down a large
/// file. Short of it, the rows on screen start from a clean state for now
/// and the event loop draws again (`catching_up`): a jump to the end of a
/// 100k-line file took 2 s in one frame.
const CATCH_UP_BUDGET: Duration = Duration::from_millis(15);

/// Colored rows kept at most; past it they're all dropped.
const MAX_CACHED_ROWS: usize = 4096;


/// `syntect`'s two states at the start of a row.
#[derive(Clone)]
struct RowState {
    parse: ParseState,
    highlight: HighlightState,
}


/// A row already colored: its highlights, and the state it started from
/// -- an edit on it starts parsing right there.
struct ColoredRow {
    start: RowState,
    highlights: Vec<Highlight>,
}


pub(super) struct SyntaxCache {
    syntax_set: Arc<SyntaxSet>,
    syntax: SyntaxReference,
    /// Shared, so a frame's `Highlighter` (which borrows it) doesn't hold
    /// `self`.
    theme: Arc<SynTheme>,
    /// The state at the start of row `k * CHECKPOINT_ROWS`, for every `k`
    /// up to how far the buffer has been parsed; `[0]` is always there.
    checkpoints: Vec<RowState>,
    /// A hash of every row the checkpoints were parsed from
    /// (`(checkpoints.len() - 1) * CHECKPOINT_ROWS` of them): how a change
    /// whose row isn't known is found (`buffer_changed`).
    row_hashes: Vec<u64>,
    /// Rows already turned into highlights, for `base`.
    rows: BTreeMap<usize, ColoredRow>,
    base: Option<Color>,
    /// The state at the start of the row after the last one colored, so
    /// scrolling down a row parses one row.
    resume: Option<(usize, RowState)>,
    /// The last frame's view was beyond the parsed part.
    catching_up: bool,
    /// `CATCH_UP_BUDGET`; tests set their own.
    budget: Duration,
}


impl SyntaxCache {
    pub(super) fn new(syntax_set: Arc<SyntaxSet>, syntax: SyntaxReference, theme: SynTheme) -> Self {
        let theme = Arc::new(theme);
        let start = clean_state(&syntax, &Highlighter::new(&theme));
        Self { syntax_set, syntax, theme, checkpoints: vec![start], row_hashes: Vec::new(), rows: BTreeMap::new(), base: None, resume: None, catching_up: false, budget: CATCH_UP_BUDGET }
    }


    /// Whether the view is still beyond what's parsed: the next frames go
    /// on parsing, and should come without waiting for a key.
    pub(super) fn catching_up(&self) -> bool {
        self.catching_up
    }


    /// `row` changed: everything from it on is stale.
    pub(super) fn row_changed(&mut self, row: usize) {
        let kept = row / CHECKPOINT_ROWS;
        self.checkpoints.truncate(kept + 1);
        self.row_hashes.truncate(kept * CHECKPOINT_ROWS);
        let mut dropped = self.rows.split_off(&row);
        if self.resume.as_ref().is_some_and(|(at, _)| *at > row) {
            self.resume = None;
        }
        // Its own start state only depends on the rows above it.
        if let Some(edited) = dropped.remove(&row) {
            self.resume = Some((row, edited.start));
        }
    }


    /// Something changed somewhere: the first row that no longer matches
    /// its hash is where -- O(that row), as `Editor::buffer_changed`'s own
    /// comparison already is. Below the hashed rows nothing is known, so
    /// all of it goes.
    pub(super) fn buffer_changed(&mut self, lines: &Lines) {
        let first_changed = self.row_hashes.iter().enumerate().position(|(row, &hash)| row_hash(row_of(lines, row)) != hash);
        self.row_changed(first_changed.unwrap_or(self.row_hashes.len()));
    }


    /// The `Highlight`s for `rows`, the rows that can be on screen, in
    /// order; text already in `base` (the editor's own color) gets none.
    pub(super) fn highlights(&mut self, lines: &Lines, rows: Range<usize>, base: Color) -> Vec<Highlight> {
        let rows = rows.start.min(lines.len())..rows.end.min(lines.len());
        if self.base != Some(base) || self.rows.len() > MAX_CACHED_ROWS {
            self.rows.clear();
            self.base = Some(base);
        }
        let theme = Arc::clone(&self.theme);
        let highlighter = Highlighter::new(&theme);
        self.catch_up(lines, rows.start, &highlighter);

        let mut highlights = Vec::new();
        if self.catching_up {
            let mut state = clean_state(&self.syntax, &highlighter);
            for row in rows {
                line_highlights(row, row_of(lines, row), &mut state, &self.syntax_set, &self.syntax, &highlighter, Some((base, &mut highlights)));
            }
            return highlights;
        }
        let mut state: Option<RowState> = None;
        for row in rows.clone() {
            if let Some(cached) = self.rows.get(&row) {
                highlights.extend(cached.highlights.iter().cloned());
                state = None;
                continue;
            }
            let state = match &mut state {
                Some(state) => state,
                none => none.insert(self.state_at(lines, row, &highlighter)),
            };
            let start = state.clone();
            let mut row_highlights = Vec::new();
            line_highlights(row, row_of(lines, row), state, &self.syntax_set, &self.syntax, &highlighter, Some((base, &mut row_highlights)));
            highlights.extend(row_highlights.iter().cloned());
            self.rows.insert(row, ColoredRow { start, highlights: row_highlights });
        }
        if let Some(state) = state {
            self.resume = Some((rows.end, state));
        }
        highlights
    }


    /// Parses on from the last checkpoint towards `row`, for at most
    /// `CATCH_UP_BUDGET`; `catching_up` says whether it got there.
    fn catch_up(&mut self, lines: &Lines, row: usize, highlighter: &Highlighter) {
        let wanted = row / CHECKPOINT_ROWS;
        let started = Instant::now();
        while self.checkpoints.len() <= wanted && started.elapsed() < self.budget {
            let mut state = self.checkpoints.last().expect("row 0's state is always kept").clone();
            let from = (self.checkpoints.len() - 1) * CHECKPOINT_ROWS;
            for parsed in from..from + CHECKPOINT_ROWS {
                let content = row_of(lines, parsed);
                self.row_hashes.push(row_hash(content));
                line_highlights(parsed, content, &mut state, &self.syntax_set, &self.syntax, highlighter, None);
            }
            self.checkpoints.push(state);
        }
        self.catching_up = self.checkpoints.len() <= wanted;
    }


    /// The state at the start of `row`, a row on screen: from the last
    /// checkpoint above it (`catch_up` got one to the view's top), or from
    /// `resume` when it's closer.
    fn state_at(&self, lines: &Lines, row: usize, highlighter: &Highlighter) -> RowState {
        let checkpoint = (row / CHECKPOINT_ROWS).min(self.checkpoints.len() - 1);
        let checkpoint_row = checkpoint * CHECKPOINT_ROWS;
        let (from, mut state) = match &self.resume {
            Some((at, state)) if (checkpoint_row..=row).contains(at) => (*at, state.clone()),
            _ => (checkpoint_row, self.checkpoints[checkpoint].clone()),
        };
        for skipped in from..row {
            line_highlights(skipped, row_of(lines, skipped), &mut state, &self.syntax_set, &self.syntax, highlighter, None);
        }
        state
    }
}


fn clean_state(syntax: &SyntaxReference, highlighter: &Highlighter) -> RowState {
    RowState { parse: ParseState::new(syntax), highlight: HighlightState::new(highlighter, ScopeStack::new()) }
}


fn row_of(lines: &Lines, row: usize) -> &[char] {
    lines.get(RowIndex::new(row)).map_or(&[], Vec::as_slice)
}


/// Parses one row on from `state`, adding its colored stretches to `out`
/// (if given, with the editor's base color) -- merged where neighbours
/// share a style, none where it's the base color, plain. A row the
/// grammar can't parse is left plain and the state starts over.
fn line_highlights(
    row: usize,
    content: &[char],
    state: &mut RowState,
    syntax_set: &SyntaxSet,
    syntax: &SyntaxReference,
    highlighter: &Highlighter,
    out: Option<(Color, &mut Vec<Highlight>)>,
) {
    // The default set expects each line with its break.
    let mut text: String = content.iter().collect();
    text.push('\n');
    let Ok(ops) = state.parse.parse_line(&text, syntax_set) else {
        *state = clean_state(syntax, highlighter);
        return;
    };
    let pieces = RangedHighlightIterator::new(&mut state.highlight, &ops, &text, highlighter);
    let Some((base, out)) = out else {
        pieces.for_each(drop);
        return;
    };
    let mut col = 0;
    for (style, piece, _) in pieces {
        let start = col;
        col += piece.chars().count();
        let end = col.min(content.len());
        let style = to_style(style);
        if start >= end || style == Style::default().fg(base) {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.start.row == row && last.end.col + 1 == start && last.style == style => last.end.col = end - 1,
            _ => out.push(Highlight::new(Index2::new(row, start), Index2::new(row, end - 1), style)),
        }
    }
}


/// `syntect`'s style as the cell's: the color, bold, italic, underline --
/// the background stays the editor's.
fn to_style(style: edtui::syntect::highlighting::Style) -> Style {
    let color = style.foreground;
    let mut result = Style::default().fg(Color::Rgb(color.r, color.g, color.b));
    for (font, modifier) in [(FontStyle::BOLD, Modifier::BOLD), (FontStyle::ITALIC, Modifier::ITALIC), (FontStyle::UNDERLINE, Modifier::UNDERLINED)] {
        if style.font_style.contains(font) {
            result = result.add_modifier(modifier);
        }
    }
    result
}


#[cfg(test)]
mod tests;
