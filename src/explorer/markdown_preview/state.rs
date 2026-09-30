use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::links::MarkdownLink;
use super::render::render_markdown;
use super::{is_markdown_file, MarkdownLine, MarkdownSpanKind};

/// `F3` on a `.md` file: the rendered preview shown beside the editor
/// (`App::markdown_edit_preview`) -- parsed lines plus scroll position.
/// Shares nothing with the editor except `reload()` after each `Ctrl+S`.
/// History: docs/history/markdown-preview.md.
pub struct MarkdownPreviewState {
    path: PathBuf,
    lines: Vec<MarkdownLine>,
    scroll: usize,
    /// Where the content was drawn last frame (`x, y, width, height`),
    /// for turning a click into a position. A tuple rather than a
    /// `ratatui::Rect`, to keep this module free of `ratatui`. `None`
    /// before the first draw.
    content_area: Option<(u16, u16, u16, u16)>,
    /// What the last `Ctrl`+click did ("opening ...", "no link here",
    /// ...), shown as the panel's bottom border title. Built from the
    /// link's label, never its URL: a truncated URL still looks like a
    /// real one, and the terminal may linkify it on its own.
    link_message: Option<String>,
    /// `(column_start, column_end, url)` link hitboxes per rendered row,
    /// set by the renderer every frame from the same wrapped rows it
    /// draws, so hit-testing matches the screen exactly.
    visible_row_links: Vec<Vec<(u16, u16, String)>>,
    /// The source line each entry of `lines` started at (parallel to
    /// `lines`), for `sync_to_editor_cursor`.
    line_source_rows: Vec<usize>,
    /// The line matching the editor's cursor, painted highlighted; `None`
    /// before the first sync or on an empty document.
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

    /// Re-reads and re-renders the file after a save. Keeps the scroll
    /// position (clamped); a read failure leaves the preview as it was.
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

    /// Set after every link-open attempt -- see the `link_message` field.
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

    /// Every link in first-appearance order -- the link search's list
    /// (`l`), the exact alternative to mouse hit-testing.
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

    /// A fixed page step (`limits().markdown_preview_page_size`) -- the
    /// preview's real height isn't fed back into this state.
    pub fn page_down(&mut self) {
        let page_size = crate::theming::config::limits().markdown_preview_page_size;
        self.scroll = (self.scroll + page_size).min(self.lines.len().saturating_sub(1));
    }

    pub fn page_up(&mut self) {
        let page_size = crate::theming::config::limits().markdown_preview_page_size;
        self.scroll = self.scroll.saturating_sub(page_size);
    }

    /// Scrolls to and highlights the line matching the editor's cursor
    /// row, placing it at `relative_position` (0.0 top .. 1.0 bottom) of
    /// the preview's visible height -- the cursor's own position on the
    /// editor's page -- so the two stay level. Called by `ui::draw` only
    /// while the editor has focus. History: docs/history/markdown-preview.md.
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

    /// The line whose source row is the last at or before `source_row`,
    /// skipping blank separators (they carry their block's closing row, often
    /// equal to its last real line). Falls back to the first content line;
    /// `None` for a document with no content.
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

    /// The URL rendered at screen `(column, row)`, if any -- read from
    /// `visible_row_links`, so it matches what's drawn even across
    /// wrapped lines. `None` outside the content area or before the
    /// first draw.
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
