use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::links::MarkdownLink;
use super::render::render_markdown;
use super::{is_markdown_file, MarkdownLine, MarkdownSpanKind, PAGE_SIZE};

/// `F3` on a `.md`/`.markdown` file (`Mode::MarkdownPreview`): the
/// file's content, already parsed into styled lines
/// (`render_markdown`), and which line is scrolled to the top of the
/// preview area. A real rendered preview (headings/bold/lists/quotes/
/// code shown structurally, not as highlighted raw text) rather than
/// just opening the file read-only in the built-in editor -- the two
/// options `TODO/viewer.md` originally left open; this is closer to
/// what "preview" usually means for Markdown specifically (a browser-
/// style rendering), and reuses none of the editor's own machinery, so
/// it stays a genuinely separate, simpler code path.
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

        Some(Self { path: path.to_path_buf(), lines: render_markdown(&content), scroll: 0, content_area: None, link_message: None, visible_row_links: Vec::new() })
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
