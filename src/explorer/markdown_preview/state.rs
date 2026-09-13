use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::links::MarkdownLink;
use super::render::render_markdown;
use super::{is_markdown_file, MarkdownLine, MarkdownSpanKind, PAGE_SIZE};

/// `F3` on a `.md`/`.markdown` file: the file's content, already parsed
/// into styled lines (`render_markdown`), and which line is scrolled to
/// the top of the preview area. Shown live alongside the built-in
/// editor for the same file (`App::markdown_edit_preview`) -- a real
/// rendered preview (headings/bold/lists/quotes/code shown
/// structurally, not as highlighted raw text) rather than a second copy
/// of the editor's own syntax-highlighted raw-text view, and reuses
/// none of the editor's own machinery, so it stays a genuinely separate,
/// simpler code path; `reload()` is the one place they touch, called
/// after every `Ctrl+S` so editing and rendering stay in sync.
pub struct MarkdownPreviewState {
    path: PathBuf,
    lines: Vec<MarkdownLine>,
    scroll: usize,
    /// Where the rendered content was last actually drawn on screen
    /// (`x, y, width, height`) -- set by `ui::markdown_preview::draw_markdown_preview`
    /// every frame, read by `handle_markdown_preview_mouse` to turn a
    /// raw terminal `(column, row)` click into "which rendered line was
    /// that." A plain tuple, not `ratatui::layout::Rect`, so this
    /// otherwise-`ratatui`-free domain module doesn't need the
    /// dependency just for this one field (same reasoning
    /// `explorer::HighlightRole` already keeps colors out of `explorer`
    /// entirely). `None` before the first draw.
    content_area: Option<(u16, u16, u16, u16)>,
    /// A one-line status describing what the *last* `Ctrl`+click
    /// actually did -- "opening https://...", "no link here", "file not
    /// found: ...", etc. (`handle_markdown_preview_mouse` sets this on
    /// every attempt, success or not). Reported directly: a click that
    /// silently does nothing (no link under the cursor, an unsupported
    /// anchor, a failed open, or even just an uncertain "did that even
    /// register?") left no way to tell what happened without checking
    /// the log file. Rendered by `ui::markdown_preview::draw_markdown_preview`
    /// as the panel's own bottom border title, so it's visible without
    /// leaving the preview.
    ///
    /// Deliberately built from a link's *label* (`links::open_link`),
    /// never its raw URL -- an earlier version embedded the full URL
    /// here, and once one was long enough to get truncated by the
    /// border's own width, the cut-off text still looked exactly like a
    /// valid, complete URL/path (e.g. ".../blob/main/CONT") -- some
    /// terminals (Windows Terminal included) auto-detect and linkify
    /// URL-shaped plain text on their own, so a user could `Ctrl`+click
    /// *that* (a click our own app never sees, handled entirely by the
    /// host terminal) and land on a real, but broken, address --
    /// reported directly as a link "looking shortened" and then 404ing.
    /// A label is normally short human text, not URL-shaped, so it
    /// can't be mistaken for a real link even when truncated.
    link_message: Option<String>,
    /// The exact `(column_start, column_end, url)` link hitboxes for
    /// each *rendered visual row*, relative to the content area's own
    /// left edge -- set by `ui::markdown_preview::draw_markdown_preview`
    /// every frame, from the very same word-wrapped rows it renders
    /// (`wrap_markdown_line`). Replaces an earlier version that
    /// approximated a click's target by mapping the raw screen row back
    /// to a *logical* line index (`scroll() + row offset`), which
    /// silently drifted once any earlier line actually wrapped --
    /// reported directly against a real link that sat right after a
    /// long, wrapping paragraph. Built from the same wrapping this
    /// module already does for rendering, so hit-testing and what's
    /// actually on screen can never disagree.
    visible_row_links: Vec<Vec<(u16, u16, String)>>,
    /// The source line (0-indexed) each entry of `lines` started at --
    /// same length as `lines`, parallel by index, produced by
    /// `render_markdown` alongside it. Only ever read through
    /// `sync_to_editor_cursor`; see its own doc comment.
    line_source_rows: Vec<usize>,
    /// Which entry of `lines` corresponds to the built-in editor's own
    /// cursor line right now, if any -- set by `sync_to_editor_cursor`,
    /// read by `ui::markdown_preview::draw_markdown_preview` to paint
    /// that line with a highlighted background. `None` before the first
    /// sync (nothing edited yet this session) or if the document is
    /// empty.
    highlighted_line: Option<usize>,
}

impl MarkdownPreviewState {
    /// `None` if `path` isn't a supported Markdown file or can't be
    /// read as UTF-8 text -- same "couldn't act on this" convention as
    /// the rest of this codebase, logged via `tracing::warn` for the
    /// read failure (not for the extension mismatch, which is routine).
    pub fn open(path: &Path) -> Option<Self> {
        if !is_markdown_file(path) {
            return None;
        }
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                warn!(path = %path.display(), %err, "failed to read markdown file for preview");
                return None;
            }
        };

        let (lines, line_source_rows) = render_markdown(&content);
        Some(Self {
            path: path.to_path_buf(),
            lines,
            scroll: 0,
            content_area: None,
            link_message: None,
            visible_row_links: Vec::new(),
            line_source_rows,
            highlighted_line: None,
        })
    }

    /// Re-reads and re-renders this preview's own file from disk --
    /// called right after a successful `Editor::save()` so the embedded
    /// preview (`App::markdown_edit_preview`) reflects what was just
    /// written, without recreating a whole new `MarkdownPreviewState`
    /// (which would also lose `scroll`). `scroll` is clamped to the
    /// freshly re-rendered line count rather than reset to `0` --
    /// editing near the end of a long document and saving shouldn't
    /// jump the preview back to the top. Leaves everything untouched on
    /// a read failure (logged, not surfaced -- same "never blocks on
    /// this" convention as the rest of this module) rather than
    /// blanking a previously-good preview over a transient disk error.
    pub fn reload(&mut self) {
        let content = match fs::read_to_string(&self.path) {
            Ok(content) => content,
            Err(err) => {
                warn!(path = %self.path.display(), %err, "failed to reload markdown preview after save");
                return;
            }
        };
        let (lines, line_source_rows) = render_markdown(&content);
        self.lines = lines;
        self.line_source_rows = line_source_rows;
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
    }

    /// Records where the content was actually drawn this frame --
    /// called once per frame from `ui::markdown_preview::draw_markdown_preview`,
    /// the only place that actually knows the real `Rect`.
    pub fn set_content_area(&mut self, x: u16, y: u16, width: u16, height: u16) {
        self.content_area = Some((x, y, width, height));
    }

    /// Records the exact link hitboxes for the rows actually rendered
    /// this frame -- see `visible_row_links`'s own field doc comment.
    pub fn set_visible_row_links(&mut self, links: Vec<Vec<(u16, u16, String)>>) {
        self.visible_row_links = links;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Set by `links::open_link` after every `Ctrl`+click/link-search
    /// `Enter` attempt, whatever the outcome -- see `link_message`'s own
    /// field doc comment for why this exists at all, and why it's built
    /// from a link's label rather than its raw URL.
    pub(super) fn set_link_message(&mut self, message: impl Into<String>) {
        self.link_message = Some(message.into());
    }

    /// The most recent `Ctrl`+click outcome, if any -- `ui::markdown_preview::draw_markdown_preview`
    /// shows this as the panel's own bottom border title.
    pub fn link_message(&self) -> Option<&str> {
        self.link_message.as_deref()
    }

    /// Every rendered line, for `ui::markdown_preview::draw_markdown_preview`
    /// to lay out -- `scroll()` says which one belongs at the top.
    pub fn lines(&self) -> &[MarkdownLine] {
        &self.lines
    }

    /// Every link in the document, in first-appearance order --
    /// `MarkdownLinkSearchState`'s own source list (`l` opens it,
    /// `open_selected_link`'s own doc comment explains why this exists
    /// alongside `Ctrl`+click at all: mouse hit-testing here is only
    /// ever an approximation, this is exact). Collected fresh from
    /// `self.lines` each time rather than cached at `open()` -- cheap,
    /// and there's no later mutation of `lines` to go stale against.
    pub fn links(&self) -> Vec<MarkdownLink> {
        self.lines
            .iter()
            .flatten()
            .filter(|span| span.kind == MarkdownSpanKind::Link)
            .filter_map(|span| span.url.clone().map(|url| MarkdownLink { label: span.text.clone(), url }))
            .collect()
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn scroll_down(&mut self) {
        if self.scroll + 1 < self.lines.len() {
            self.scroll += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn page_down(&mut self) {
        self.scroll = (self.scroll + PAGE_SIZE).min(self.lines.len().saturating_sub(1));
    }

    pub fn page_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(PAGE_SIZE);
    }

    /// Scrolls to and highlights the rendered line corresponding to the
    /// built-in editor's own cursor row (`Editor::cursor_row`) -- called
    /// every frame while a linked editor has keyboard focus (`ui::draw`,
    /// only while `App::active == 0`, so a manual scroll through the
    /// preview itself -- `App::active == 1` -- isn't immediately
    /// overwritten).
    ///
    /// `relative_position` (`0.0` = the matched line lands at the very
    /// top of the preview's own visible area, `1.0` = the very bottom)
    /// is where the cursor currently sits within the *editor's* own
    /// visible page -- `ui::draw` computes it from `Editor::cursor_row`/
    /// `viewport_top_row`, this method just places the matched preview
    /// line at the same fraction of `visible_height` (the preview's own
    /// content row count). Requested directly, twice: first "сделать
    /// одновременной... выделить строку", then, once a simpler always-
    /// top-aligned version was actually seen in use, "можно их примерно
    /// на одном уровне держать по странице, если редактирование в
    /// середине страницы, то и превью в том же месте" -- top-aligning
    /// technically kept them in sync but put the highlighted line at a
    /// different *screen row* than the cursor's own, which is what
    /// "held at the same level" actually meant.
    pub fn sync_to_editor_cursor(&mut self, source_row: usize, relative_position: f64, visible_height: usize) {
        let Some(line_index) = self.line_for_source_row(source_row) else {
            return;
        };
        self.highlighted_line = Some(line_index);
        let offset = (relative_position.clamp(0.0, 1.0) * visible_height as f64).round() as usize;
        self.scroll = line_index.saturating_sub(offset);
    }

    /// Which entry of `lines()` is currently highlighted as "the line
    /// being edited," if any -- see `highlighted_line`'s own field doc
    /// comment.
    pub fn highlighted_line(&self) -> Option<usize> {
        self.highlighted_line
    }

    /// The index into `lines()` whose own source line is the closest
    /// one at or before `source_row` -- skips blank separator lines
    /// entirely (`render_markdown` stamps each with whatever row the
    /// block it followed *closed* on, which is often the exact same row
    /// as that block's own last real content line -- including one
    /// would make it the ambiguous, wrong pick whenever a scan for "the
    /// last line at or before this row" reaches it first). Falls back
    /// to the first real content line if `source_row` sits before
    /// everything (e.g. the cursor is on a blank line at the very top);
    /// `None` only when the document has no content lines at all.
    fn line_for_source_row(&self, source_row: usize) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (index, line) in self.lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            if self.line_source_rows[index] <= source_row {
                best = Some(index);
            }
        }
        best.or_else(|| self.lines.iter().position(|line| !line.is_empty()))
    }

    /// The URL of the link actually rendered at screen position
    /// `(column, row)`, if any -- `None` if the click landed outside
    /// the content area entirely, on a row with no link there, or
    /// before the first frame has actually drawn anything
    /// (`content_area`/`visible_row_links` still empty).
    ///
    /// Exact, not approximated: reads straight from `visible_row_links`
    /// (`set_visible_row_links`'s own doc comment), which
    /// `ui::markdown_preview::draw_markdown_preview` rebuilds every
    /// frame from the *same* word-wrapped rows it actually renders
    /// (`wrap_markdown_line`) -- so this can never disagree with what's
    /// really on screen, unlike an earlier version that mapped a screen
    /// row straight back to a *logical* line index (`scroll() + row
    /// offset`) without accounting for word-wrap at all, and drifted
    /// once any earlier line had wrapped (reported directly against a
    /// real link that sat right after a long, wrapping paragraph).
    pub(crate) fn link_at(&self, column: u16, row: u16) -> Option<&str> {
        let (x, y, width, height) = self.content_area?;
        if column < x || column >= x + width || row < y || row >= y + height {
            return None;
        }
        let relative_row = (row - y) as usize;
        let relative_col = column - x;
        self.visible_row_links.get(relative_row)?.iter().find(|(start, end, _)| relative_col >= *start && relative_col < *end).map(|(_, _, url)| url.as_str())
    }
}
