use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::execute;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use tracing::{debug, warn};

use crate::app::{App, Mode};
use super::system_open;

/// A fixed scroll step for `PageUp`/`PageDown` -- there's no per-frame
/// feedback loop threading the preview area's real visible height back
/// into `MarkdownPreviewState` (unlike `Panel`'s own `set_visible_rows`,
/// fed back through `ui::draw`'s return value), so this is a reasonable
/// approximation rather than an exact page, same tradeoff accepted
/// elsewhere in this codebase for things not worth that plumbing.
const PAGE_SIZE: usize = 15;

/// Whether `path` is a file `F3`'s Markdown preview knows how to open.
pub fn is_markdown_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}


/// One styled run of text within a rendered line -- domain-level only,
/// no `ratatui` dependency here (same split `explorer::HighlightRole`
/// already has from its own color mapping in `ui.rs`): `ui::markdown_preview`
/// maps each `MarkdownSpanKind` to a real `Style` using the active
/// `Theme`, this module only decides *what* a span structurally is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownSpan {
    pub text: String,
    pub kind: MarkdownSpanKind,
    /// The link target, for a `MarkdownSpanKind::Link` span -- `None`
    /// for every other kind. Kept even though the URL itself is never
    /// shown in the rendered text (a preview, not a browser) so a mouse
    /// click can actually open it (`handle_markdown_preview_mouse`).
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownSpanKind {
    Plain,
    /// `1..=6`, the heading level (`h1`..`h6`).
    Heading(u8),
    Bold,
    Italic,
    /// Both inline `` `code` `` spans and fenced/indented code block lines
    /// -- rendered the same way (a distinct, monospace-reading color);
    /// this is a preview, not a second syntax-highlighted editor.
    Code,
    Link,
    Quote,
    /// A horizontal rule (`---`), rendered as its own full-width line.
    Rule,
}

pub type MarkdownLine = Vec<MarkdownSpan>;


/// One link found in the document -- `label` is its own rendered text
/// (what `MarkdownLinkSearchState` searches against, alongside `url`
/// itself), `url` its raw target as written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    pub label: String,
    pub url: String,
}


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
    link_message: Option<String>,
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

        Some(Self { path: path.to_path_buf(), lines: render_markdown(&content), scroll: 0, content_area: None, link_message: None })
    }

    /// Records where the content was actually drawn this frame --
    /// called once per frame from `ui::markdown_preview::draw_markdown_preview`,
    /// the only place that actually knows the real `Rect`.
    pub fn set_content_area(&mut self, x: u16, y: u16, width: u16, height: u16) {
        self.content_area = Some((x, y, width, height));
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Set by `handle_markdown_preview_mouse` after every `Ctrl`+click
    /// attempt, whatever the outcome -- see `link_message`'s own field
    /// doc comment for why this exists at all.
    pub fn set_link_message(&mut self, message: impl Into<String>) {
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

    /// The URL of the first link on whichever rendered line contains
    /// screen position `(column, row)`, if any -- `None` if the click
    /// landed outside the content area entirely, on a line with no
    /// link at all, or before the first frame has actually drawn
    /// anything (`content_area` still `None`).
    ///
    /// Deliberately line-level, not column-precise: `ratatui`'s
    /// `Paragraph` word-wraps a long logical line into several visual
    /// rows on its own, and this maps a screen row straight back to a
    /// logical-line index (`scroll() + row offset`) without replicating
    /// that wrap -- correct as long as *nothing above the clicked line*
    /// has wrapped, but drifts (off by however many extra rows that
    /// wrap consumed) once it has, which is common enough in a real
    /// document to make `Ctrl`+click alone unreliable in practice
    /// (reported directly). Reimplementing `ratatui`'s own wrapping
    /// algorithm just to fix mouse hit-testing wasn't judged worth it
    /// once there was a genuinely exact alternative available instead:
    /// `l` opens `Mode::MarkdownLinkSearch`, which works from the same
    /// parsed `MarkdownLink` list (`links()`) rather than screen
    /// position at all, so it's correct regardless of wrapping.
    /// `Ctrl`+click stays as a quick option for a short, unwrapped line
    /// (the common case), not the only way to reach a link.
    fn link_at(&self, column: u16, row: u16) -> Option<&str> {
        let (x, y, width, height) = self.content_area?;
        if column < x || column >= x + width || row < y || row >= y + height {
            return None;
        }
        let line_index = self.scroll + (row - y) as usize;
        self.lines.get(line_index)?.iter().find_map(|span| span.url.as_deref())
    }
}


/// Walks `content`'s own `pulldown_cmark` event stream into a flat list
/// of styled lines -- deliberately not a full CommonMark-to-terminal
/// renderer (tables, footnotes, task-list checkboxes, and images all
/// fall through the catch-all `_ => {}` arm below and are silently
/// dropped rather than misrendered), just the common constructs a real
/// README/notes file actually uses: headings, bold/italic, inline and
/// fenced code, block quotes, ordered/unordered (possibly nested)
/// lists, links (shown as their own link-colored text, without the
/// underlying URL in the *displayed* text -- a preview, not a browser
/// -- but the URL is still kept on the span itself, so a mouse click
/// can open it, `handle_markdown_preview_mouse`), and horizontal rules.
/// Loose lists (CommonMark wrapping each item's content in its own
/// `Paragraph`) pick up an extra blank line between items, a known,
/// minor cosmetic gap rather than tracking list-tightness separately.
fn render_markdown(content: &str) -> Vec<MarkdownLine> {
    let mut lines: Vec<MarkdownLine> = Vec::new();
    let mut current: MarkdownLine = Vec::new();

    let mut bold_depth = 0usize;
    let mut italic_depth = 0usize;
    let mut quote_depth = 0usize;
    let mut heading: Option<u8> = None;
    let mut in_code_block = false;
    // The innermost open link's own URL, if any -- links don't nest in
    // real Markdown, but a plain `Option` (rather than a depth counter
    // like `bold_depth`/`italic_depth`) is exactly what's needed to
    // stamp onto every span produced while inside one.
    let mut link_url: Option<String> = None;
    // One entry per currently-open list, innermost last -- `.0` is
    // whether it's ordered, `.1` the next item number to hand out.
    let mut list_stack: Vec<(bool, u64)> = Vec::new();

    let flush = |current: &mut MarkdownLine, lines: &mut Vec<MarkdownLine>| {
        if !current.is_empty() {
            lines.push(std::mem::take(current));
        }
    };
    let span_kind = |heading: Option<u8>, bold: usize, italic: usize, link: bool, quote: usize| {
        if let Some(level) = heading {
            MarkdownSpanKind::Heading(level)
        } else if link {
            MarkdownSpanKind::Link
        } else if quote > 0 {
            MarkdownSpanKind::Quote
        } else if bold > 0 {
            MarkdownSpanKind::Bold
        } else if italic > 0 {
            MarkdownSpanKind::Italic
        } else {
            MarkdownSpanKind::Plain
        }
    };

    for event in Parser::new(content) {
        match event {
            Event::End(TagEnd::Paragraph) => {
                flush(&mut current, &mut lines);
                lines.push(Vec::new());
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut lines);
                heading = Some(heading_level_number(level));
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut current, &mut lines);
                heading = None;
                lines.push(Vec::new());
            }
            Event::Start(Tag::Strong) => bold_depth += 1,
            Event::End(TagEnd::Strong) => bold_depth = bold_depth.saturating_sub(1),
            Event::Start(Tag::Emphasis) => italic_depth += 1,
            Event::End(TagEnd::Emphasis) => italic_depth = italic_depth.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => link_url = Some(dest_url.into_string()),
            Event::End(TagEnd::Link) => link_url = None,
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut current, &mut lines);
                quote_depth = quote_depth.saturating_sub(1);
                lines.push(Vec::new());
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut current, &mut lines);
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                lines.push(Vec::new());
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut current, &mut lines);
                list_stack.push((start.is_some(), start.unwrap_or(1)));
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
                lines.push(Vec::new());
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut lines);
                let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                let prefix = match list_stack.last_mut() {
                    Some((true, counter)) => {
                        let n = *counter;
                        *counter += 1;
                        format!("{indent}{n}. ")
                    }
                    _ => format!("{indent}- "),
                };
                current.push(MarkdownSpan { text: prefix, kind: MarkdownSpanKind::Plain, url: None });
            }
            Event::End(TagEnd::Item) => flush(&mut current, &mut lines),
            Event::Rule => {
                flush(&mut current, &mut lines);
                lines.push(vec![MarkdownSpan { text: "\u{2500}".repeat(40), kind: MarkdownSpanKind::Rule, url: None }]);
                lines.push(Vec::new());
            }
            Event::Text(text) => {
                if in_code_block {
                    flush(&mut current, &mut lines);
                    // A fenced/indented code block's whole body arrives
                    // as one `Text` event with embedded newlines --
                    // `split('\n')` on text ending in `\n` leaves one
                    // spurious empty trailing element, dropped below.
                    let mut code_lines: Vec<&str> = text.split('\n').collect();
                    if text.ends_with('\n') {
                        code_lines.pop();
                    }
                    for code_line in code_lines {
                        lines.push(vec![MarkdownSpan { text: code_line.to_string(), kind: MarkdownSpanKind::Code, url: None }]);
                    }
                } else {
                    let kind = span_kind(heading, bold_depth, italic_depth, link_url.is_some(), quote_depth);
                    current.push(MarkdownSpan { text: text.into_string(), kind, url: link_url.clone() });
                }
            }
            Event::Code(text) => current.push(MarkdownSpan { text: text.into_string(), kind: MarkdownSpanKind::Code, url: None }),
            Event::SoftBreak => current.push(MarkdownSpan { text: " ".to_string(), kind: MarkdownSpanKind::Plain, url: None }),
            Event::HardBreak => flush(&mut current, &mut lines),
            _ => {}
        }
    }
    flush(&mut current, &mut lines);

    // A tidier ending than however many blank "paragraph/heading/list
    // just closed" lines happened to accumulate at the very end.
    while matches!(lines.last(), Some(line) if line.is_empty()) {
        lines.pop();
    }

    lines
}

fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}


/// `F3`: opens `Mode::MarkdownPreview` for the active panel's own
/// selected entry, if it's a `.md`/`.markdown` file -- a silent no-op
/// otherwise. Switches the *right* panel to active (`app.active = 1`),
/// same convention `image_preview::open_preview` already established
/// for `F3`: that's the panel whose own area now shows the preview
/// instead of a file listing.
///
/// Also turns on mouse capture (`EnableMouseCapture`) -- requested
/// directly, so a link can be opened with a click (touchpad or mouse),
/// not just read. Scoped to exactly this preview session rather than
/// enabled for the whole app: mouse capture takes over the terminal's
/// own native text selection, which would otherwise get in the way of
/// copying paths/output with the mouse everywhere else in this file
/// manager -- `handle_markdown_preview_key` turns it back off the
/// moment the preview closes. A failed `execute!` (a real write error
/// to stdout) is logged, not surfaced -- same "never blocks on this"
/// rule as every other terminal-control call in this codebase; the
/// preview still opens either way, just without clickable links.
pub fn open_preview(app: &mut App) {
    let panel = app.active_panel();
    let Some(path) = panel.selected_path() else {
        return;
    };

    let Some(state) = MarkdownPreviewState::open(&path) else {
        return;
    };

    match execute!(std::io::stdout(), EnableMouseCapture) {
        // `app.mouse_capture_enabled` is what tells `main.rs::restore_terminal`
        // it's safe to send `DisableMouseCapture` at all when the app
        // exits -- only set once this actually succeeded, never
        // unconditionally (see its own doc comment for the crash that
        // came from assuming it was always safe).
        Ok(()) => app.mouse_capture_enabled = true,
        Err(err) => warn!(%err, "failed to enable mouse capture for the markdown preview"),
    }

    app.active = 1;
    app.mode = Mode::MarkdownPreview(state);
}


/// Key handling while `Mode::MarkdownPreview` is showing: `Up`/`Down`
/// scroll one line, `PageUp`/`PageDown` a fixed chunk (`PAGE_SIZE`);
/// `l` opens the keyboard-driven link search (`Mode::MarkdownLinkSearch`,
/// `open_link_search`'s own doc comment); `Esc` or `F3` again closes
/// back to `Mode::Browsing` -- and turns mouse capture back off, if
/// `open_preview` actually turned it on (`app.mouse_capture_enabled` --
/// own doc comment on `App` explains why this is checked rather than
/// assumed).
pub fn handle_markdown_preview_key(app: &mut App, key: KeyEvent) {
    if let KeyCode::Char('l' | 'L') = key.code {
        open_link_search(app);
        return;
    }

    let Mode::MarkdownPreview(state) = &mut app.mode else {
        return;
    };

    match key.code {
        KeyCode::Up => state.scroll_up(),
        KeyCode::Down => state.scroll_down(),
        KeyCode::PageUp => state.page_up(),
        KeyCode::PageDown => state.page_down(),
        KeyCode::Esc | KeyCode::F(3) => {
            if app.mouse_capture_enabled {
                if let Err(err) = execute!(std::io::stdout(), DisableMouseCapture) {
                    warn!(%err, "failed to disable mouse capture after closing the markdown preview");
                }
                app.mouse_capture_enabled = false;
            }
            app.mode = Mode::Browsing;
        }
        _ => {}
    }
}


/// `l` on the Markdown preview: opens the keyboard-driven link browser
/// (`Mode::MarkdownLinkSearch`), requested
/// directly as a more reliable alternative to `Ctrl`+click
/// (`link_at`'s own doc comment on why that's only ever an
/// approximation once word-wrap is involved) -- exact by construction,
/// since it works from the same parsed `MarkdownLink` list
/// `resolve_link_target` already trusts, not from guessing which
/// on-screen row a click landed on. A no-op if the document has no
/// links at all (nothing to search).
fn open_link_search(app: &mut App) {
    if !matches!(&app.mode, Mode::MarkdownPreview(_)) {
        return;
    }
    let Mode::MarkdownPreview(state) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
        unreachable!("just matched Mode::MarkdownPreview above");
    };

    let links = state.links();
    if links.is_empty() {
        app.mode = Mode::MarkdownPreview(state);
        return;
    }

    app.mode = Mode::MarkdownLinkSearch(state, MarkdownLinkSearchState::new(links));
}


/// `Mode::MarkdownLinkSearch`: a filterable list of every link in the
/// document (`MarkdownPreviewState::links`), opened via `l` on the
/// preview -- typing narrows `filtered()` to labels/URLs containing the
/// typed text (case-insensitively), `Up`/`Down` move within it, `Enter`
/// opens the highlighted one (`open_link`, the same resolve-and-open
/// logic `Ctrl`+click uses). Deliberately simple text entry (append/
/// backspace only, no cursor movement or selection) -- same scope as
/// the editor's own `Ctrl+F` search box (`editor.rs::search_push_char`/
/// `_pop_char`), which this is modeled on: a short filter query has no
/// real need for the fuller `text_field.rs` machinery other popups in
/// this app use for actual data entry.
pub struct MarkdownLinkSearchState {
    links: Vec<MarkdownLink>,
    query: String,
    selected: usize,
}

impl MarkdownLinkSearchState {
    pub fn new(links: Vec<MarkdownLink>) -> Self {
        Self { links, query: String::new(), selected: 0 }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Links whose label or URL contains `query`, case-insensitively,
    /// in original document order -- what's actually shown in the list.
    /// Recomputed on demand rather than cached alongside `query` --
    /// this list is short enough (a real document's own link count)
    /// that re-filtering on every keystroke is not worth tracking
    /// separately.
    pub fn filtered(&self) -> Vec<&MarkdownLink> {
        let query = self.query.to_lowercase();
        self.links.iter().filter(|link| query.is_empty() || link.label.to_lowercase().contains(&query) || link.url.to_lowercase().contains(&query)).collect()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    pub fn move_down(&mut self) {
        let len = self.filtered().len();
        if len > 0 && self.selected + 1 < len {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// The link currently highlighted in the filtered list, if any --
    /// `None` for an empty list (nothing matched the typed query).
    pub fn selected_link(&self) -> Option<MarkdownLink> {
        self.filtered().get(self.selected).map(|link| (*link).clone())
    }
}


/// Key handling on `Mode::MarkdownLinkSearch`: typing filters the list,
/// `Up`/`Down` move within it, `Enter` opens the highlighted link
/// (`open_link`) and returns to `Mode::MarkdownPreview`, `Esc` cancels
/// back to it unchanged.
pub fn handle_markdown_link_search_key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        let Mode::MarkdownLinkSearch(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            return;
        };
        app.mode = Mode::MarkdownPreview(state);
        return;
    }

    if key.code == KeyCode::Enter {
        let Mode::MarkdownLinkSearch(_, search) = &app.mode else {
            return;
        };
        let selected = search.selected_link();
        let Mode::MarkdownLinkSearch(mut state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
            unreachable!("just matched Mode::MarkdownLinkSearch above");
        };
        if let Some(link) = selected {
            open_link(&mut state, &link.url);
        }
        app.mode = Mode::MarkdownPreview(state);
        return;
    }

    let Mode::MarkdownLinkSearch(_, search) = &mut app.mode else {
        return;
    };
    match key.code {
        KeyCode::Up => search.move_up(),
        KeyCode::Down => search.move_down(),
        KeyCode::Backspace => search.pop_char(),
        KeyCode::Char(c) => search.push_char(c),
        _ => {}
    }
}


/// Resolves a Markdown link's raw `url` (as written in the source file)
/// into something actually safe to hand to `system_open::open` --
/// `None` for anything that isn't, rather than passing it through
/// blindly. Reported directly as a real bug: a raw in-document anchor
/// (`#section`, common in a README's own table of contents) or a bare
/// relative reference `explorer.exe` doesn't recognize as either a URL
/// or an existing path makes it silently fall back to opening its own
/// default location instead (which, depending on the user's own
/// Windows configuration, isn't necessarily even a fixed, predictable
/// folder -- reported as landing in a OneDrive-redirected `Documents`)
/// -- confusing, and nothing to do with the link that was actually
/// clicked.
///
/// - `#fragment` alone -- an in-document anchor. Not supported yet
///   (this preview has no heading-to-line index to jump through); `None`
///   rather than trying to open it as anything else.
/// - A real absolute URL (contains `://`, or a `mailto:` link) --
///   returned as-is; `system_open::open` already hands these straight
///   to the OS's own default handler (browser/mail client).
/// - Anything else is treated as a relative reference to another file
///   in the same project, resolved against `markdown_dir` (the
///   previewed file's own directory) -- a leading `#fragment` on such a
///   link (`file.md#section`) is stripped first, then `None` unless the
///   resulting path actually exists on disk, so a broken or
///   not-yet-supported reference never gets handed to the OS as a
///   guess.
fn resolve_link_target(url: &str, markdown_dir: &Path) -> Option<PathBuf> {
    if url.starts_with('#') {
        return None;
    }
    if url.contains("://") || url.starts_with("mailto:") {
        return Some(PathBuf::from(url));
    }

    let relative = url.split('#').next().unwrap_or(url);
    if relative.is_empty() {
        return None;
    }
    let target = markdown_dir.join(relative);
    target.exists().then_some(target)
}

/// Resolves and opens `url` (a link's raw target, from either
/// `Ctrl`+click or `Mode::MarkdownLinkSearch`'s own `Enter`), and always
/// leaves a visible record of what happened on `state`
/// (`set_link_message`) -- "Opened: ...", "Failed to open ...: ...", or
/// "Can't open yet: ..." for anything `resolve_link_target` won't
/// vouch for. Requested directly after a first version of `Ctrl`+click
/// left no visible trace at all once it started correctly refusing to
/// open anchors/missing files -- indistinguishable from the click not
/// registering.
fn open_link(state: &mut MarkdownPreviewState, url: &str) {
    let markdown_dir = state.path().parent().map(Path::to_path_buf).unwrap_or_default();
    match resolve_link_target(url, &markdown_dir) {
        Some(target) => {
            debug!(url, target = %target.display(), "markdown preview: opening link");
            match system_open::open(&target) {
                Ok(_) => state.set_link_message(format!("Opened: {url}")),
                Err(err) => {
                    warn!(url, target = %target.display(), %err, "failed to open a markdown preview link");
                    state.set_link_message(format!("Failed to open {url}: {err}"));
                }
            }
        }
        None => {
            debug!(url, "markdown preview: link has no openable target (anchor, or relative file not found)");
            state.set_link_message(format!("Can't open yet: {url} (in-document anchor, or file not found)"));
        }
    }
}

/// Mouse handling while `Mode::MarkdownPreview` is showing:
/// `Ctrl`+left-click on a rendered line containing a link opens it with
/// the OS's own default handler (`system_open::open` -- built for
/// opening a file/directory in the OS file manager, but its per-OS
/// commands (`explorer`/`open`/`xdg-open`) already hand a URL straight
/// to the default browser just as well, since none of them care whether
/// their argument is a filesystem path or a URL) -- but only once
/// `resolve_link_target` has actually turned the link's raw text into
/// something worth opening (see its own doc comment for the real bug
/// this guards against). Requires `Ctrl` (rather than a plain click) so
/// an ordinary click/drag can still be used for the terminal's own
/// purposes without every click on a link line firing navigation --
/// the same convention most GUI terminals and editors use for
/// clickable links (VS Code's integrated terminal, iTerm2, ...). The
/// scroll wheel also scrolls the preview, same step as `Up`/`Down`,
/// with no `Ctrl` needed -- a natural, essentially-free addition once
/// mouse events were flowing through at all for the click-a-link
/// feature actually requested. Anything else (a plain click, a
/// right/middle click, mouse movement, drag) is ignored.
pub fn handle_markdown_preview_mouse(app: &mut App, mouse: MouseEvent) {
    let Mode::MarkdownPreview(state) = &mut app.mode else {
        return;
    };

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if mouse.modifiers.contains(KeyModifiers::CONTROL) => {
            debug!(column = mouse.column, row = mouse.row, "markdown preview: ctrl+click");
            let Some(url) = state.link_at(mouse.column, mouse.row) else {
                debug!("markdown preview: ctrl+click landed on a line with no link");
                state.set_link_message("Ctrl+click: no link on this line (try 'l' to search links instead)");
                return;
            };
            let url = url.to_string();
            open_link(state, &url);
        }
        MouseEventKind::ScrollDown => state.scroll_down(),
        MouseEventKind::ScrollUp => state.scroll_up(),
        _ => {}
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    #[test]
    fn is_markdown_file_recognizes_md_and_markdown_case_insensitively() {
        assert!(is_markdown_file(Path::new("readme.md")));
        assert!(is_markdown_file(Path::new("README.MD")));
        assert!(is_markdown_file(Path::new("notes.markdown")));
    }

    #[test]
    fn is_markdown_file_rejects_other_extensions() {
        assert!(!is_markdown_file(Path::new("photo.png")));
        assert!(!is_markdown_file(Path::new("no_extension")));
    }

    mod render_markdown_tests {
        use super::*;

        fn line_text(line: &MarkdownLine) -> String {
            line.iter().map(|span| span.text.as_str()).collect()
        }

        #[test]
        fn renders_a_heading_with_its_own_level() {
            let lines = render_markdown("# Title\n");
            let heading = lines.iter().find(|line| !line.is_empty()).unwrap();
            assert_eq!(line_text(heading), "Title");
            assert_eq!(heading[0].kind, MarkdownSpanKind::Heading(1));
        }

        #[test]
        fn renders_bold_and_italic_spans() {
            let lines = render_markdown("plain **bold** and *italic*\n");
            let line = &lines[0];
            let bold = line.iter().find(|s| s.text == "bold").unwrap();
            assert_eq!(bold.kind, MarkdownSpanKind::Bold);
            let italic = line.iter().find(|s| s.text == "italic").unwrap();
            assert_eq!(italic.kind, MarkdownSpanKind::Italic);
        }

        #[test]
        fn renders_inline_code_as_a_code_span() {
            let lines = render_markdown("run `cargo test` now\n");
            let code = lines[0].iter().find(|s| s.text == "cargo test").unwrap();
            assert_eq!(code.kind, MarkdownSpanKind::Code);
        }

        #[test]
        fn renders_a_fenced_code_block_as_code_lines() {
            let lines = render_markdown("```\nfn main() {}\nlet x = 1;\n```\n");
            let code_lines: Vec<&MarkdownLine> = lines.iter().filter(|line| line.iter().any(|s| s.kind == MarkdownSpanKind::Code)).collect();
            assert_eq!(code_lines.len(), 2);
            assert_eq!(line_text(code_lines[0]), "fn main() {}");
            assert_eq!(line_text(code_lines[1]), "let x = 1;");
        }

        #[test]
        fn renders_unordered_list_items_with_a_bullet_prefix() {
            let lines = render_markdown("- one\n- two\n");
            let items: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(line_text).collect();
            assert_eq!(items, vec!["- one", "- two"]);
        }

        #[test]
        fn renders_ordered_list_items_with_their_own_numbers() {
            let lines = render_markdown("1. first\n2. second\n");
            let items: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(line_text).collect();
            assert_eq!(items, vec!["1. first", "2. second"]);
        }

        #[test]
        fn renders_nested_list_items_indented() {
            let lines = render_markdown("- top\n  - nested\n");
            let items: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(line_text).collect();
            assert_eq!(items[0], "- top");
            assert!(items[1].starts_with("  - "), "nested item should be indented: {items:?}");
        }

        #[test]
        fn renders_a_blockquote_with_the_quote_kind() {
            let lines = render_markdown("> quoted text\n");
            let line = lines.iter().find(|l| !l.is_empty()).unwrap();
            assert_eq!(line[0].kind, MarkdownSpanKind::Quote);
        }

        #[test]
        fn renders_a_horizontal_rule_as_its_own_line() {
            let lines = render_markdown("above\n\n---\n\nbelow\n");
            assert!(lines.iter().any(|l| l.len() == 1 && l[0].kind == MarkdownSpanKind::Rule));
        }

        #[test]
        fn trims_trailing_blank_lines() {
            let lines = render_markdown("one paragraph\n");
            assert!(!lines.is_empty());
            assert!(!lines.last().unwrap().is_empty(), "should not end on a blank line");
        }
    }

    mod markdown_preview_state_tests {
        use super::*;

        #[test]
        fn open_fails_for_a_non_markdown_path() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("notes.txt");
            fs::write(&path, "hi").unwrap();

            assert!(MarkdownPreviewState::open(&path).is_none());
        }

        #[test]
        fn open_reads_and_renders_a_real_file() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "# Title\n\nSome text.\n").unwrap();

            let state = MarkdownPreviewState::open(&path).unwrap();

            assert_eq!(state.path(), path);
            assert!(!state.lines().is_empty());
        }

        #[test]
        fn scroll_down_stops_at_the_last_line() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "a\n\nb\n").unwrap();
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            let last = state.lines().len() - 1;

            for _ in 0..last + 5 {
                state.scroll_down();
            }

            assert_eq!(state.scroll(), last);
        }

        #[test]
        fn scroll_up_stops_at_zero() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "a\n").unwrap();
            let mut state = MarkdownPreviewState::open(&path).unwrap();

            state.scroll_up();

            assert_eq!(state.scroll(), 0);
        }

        /// The actual point of the whole click-a-link feature: a click
        /// landing on the rendered line holding the link finds its URL.
        #[test]
        fn link_at_finds_the_url_on_the_clicked_line() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(0, 0, 80, 24);

            assert_eq!(state.link_at(0, 0), Some("https://anthropic.com"));
        }

        #[test]
        fn link_at_is_none_before_the_first_draw_has_recorded_a_content_area() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
            let state = MarkdownPreviewState::open(&path).unwrap();

            assert_eq!(state.link_at(0, 0), None);
        }

        #[test]
        fn link_at_is_none_outside_the_content_area() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(10, 10, 20, 5);

            assert_eq!(state.link_at(0, 0), None, "click landed to the left of/above the content area");
        }

        #[test]
        fn link_at_is_none_on_a_line_with_no_link() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "just plain text\n").unwrap();
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(0, 0, 80, 24);

            assert_eq!(state.link_at(0, 0), None);
        }

        #[test]
        fn link_at_accounts_for_scroll_offset() {
            let dir = unique_scratch_dir("markdown-preview");
            let path = dir.join("readme.md");
            fs::write(&path, "line one\n\n[link](https://example.com)\n").unwrap();
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(0, 0, 80, 24);
            state.scroll_down();
            state.scroll_down();

            assert_eq!(state.link_at(0, 0), Some("https://example.com"), "row 0 on screen should now be the scrolled-to line holding the link");
        }
    }

    mod open_preview_tests {
        use super::*;
        use crate::test_support::test_app;

        #[test]
        fn opens_the_preview_and_switches_the_right_panel_active() {
            let dir = unique_scratch_dir("markdown-preview-open");
            fs::write(dir.join("readme.md"), "# hi\n").unwrap();
            let mut app = test_app(dir.clone());
            app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "readme.md").unwrap();

            open_preview(&mut app);

            assert!(matches!(app.mode, Mode::MarkdownPreview(_)));
            assert_eq!(app.active, 1, "the right panel should become active");
        }

        #[test]
        fn is_a_noop_on_a_non_markdown_file() {
            let dir = unique_scratch_dir("markdown-preview-open");
            fs::write(dir.join("notes.txt"), "hi").unwrap();
            let mut app = test_app(dir);
            app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "notes.txt").unwrap();

            open_preview(&mut app);

            assert!(matches!(app.mode, Mode::Browsing));
        }
    }

    mod handle_markdown_preview_key_tests {
        use super::*;
        use crate::test_support::{key, test_app};

        fn app_in_preview() -> App {
            let dir = unique_scratch_dir("markdown-preview-keys");
            let path = dir.join("readme.md");
            fs::write(&path, "line one\n\nline two\n\nline three\n").unwrap();
            let mut app = test_app(dir);
            app.mode = Mode::MarkdownPreview(MarkdownPreviewState::open(&path).unwrap());
            app
        }

        #[test]
        fn down_scrolls_forward() {
            let mut app = app_in_preview();

            handle_markdown_preview_key(&mut app, key(KeyCode::Down));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            assert_eq!(state.scroll(), 1);
        }

        #[test]
        fn esc_closes_the_preview() {
            let mut app = app_in_preview();

            handle_markdown_preview_key(&mut app, key(KeyCode::Esc));

            assert!(matches!(app.mode, Mode::Browsing));
        }

        #[test]
        fn f3_again_also_closes_the_preview() {
            let mut app = app_in_preview();

            handle_markdown_preview_key(&mut app, key(KeyCode::F(3)));

            assert!(matches!(app.mode, Mode::Browsing));
        }

        #[test]
        fn is_a_noop_outside_markdown_preview_mode() {
            let mut app = app_in_preview();
            app.mode = Mode::Browsing;

            handle_markdown_preview_key(&mut app, key(KeyCode::Down));

            assert!(matches!(app.mode, Mode::Browsing));
        }

        /// The actual point of the whole keyboard-search feature: `l`
        /// opens it, holding the menu it was pressed from so `Esc`/
        /// `Enter` can hand it straight back.
        #[test]
        fn l_opens_the_link_search_when_the_document_has_links() {
            let dir = unique_scratch_dir("markdown-preview-keys");
            let path = dir.join("readme.md");
            fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
            let mut app = test_app(dir);
            app.mode = Mode::MarkdownPreview(MarkdownPreviewState::open(&path).unwrap());

            handle_markdown_preview_key(&mut app, key(KeyCode::Char('l')));

            assert!(matches!(app.mode, Mode::MarkdownLinkSearch(..)));
        }

        #[test]
        fn l_is_a_noop_when_the_document_has_no_links() {
            let mut app = app_in_preview();

            handle_markdown_preview_key(&mut app, key(KeyCode::Char('l')));

            assert!(matches!(app.mode, Mode::MarkdownPreview(_)), "should stay put, nothing to search");
        }
    }

    mod markdown_link_search_state_tests {
        use super::*;

        fn links() -> Vec<MarkdownLink> {
            vec![
                MarkdownLink { label: "Anthropic".to_string(), url: "https://anthropic.com".to_string() },
                MarkdownLink { label: "Contributing".to_string(), url: "CONTRIBUTING.md".to_string() },
            ]
        }

        #[test]
        fn filtered_returns_everything_with_an_empty_query() {
            let search = MarkdownLinkSearchState::new(links());
            assert_eq!(search.filtered().len(), 2);
        }

        #[test]
        fn filtered_matches_the_label_case_insensitively() {
            let mut search = MarkdownLinkSearchState::new(links());
            for c in "anthro".chars() {
                search.push_char(c);
            }
            let filtered = search.filtered();
            assert_eq!(filtered.len(), 1);
            assert_eq!(filtered[0].label, "Anthropic");
        }

        #[test]
        fn filtered_also_matches_the_url() {
            let mut search = MarkdownLinkSearchState::new(links());
            for c in "contributing.md".chars() {
                search.push_char(c);
            }
            assert_eq!(search.filtered().len(), 1);
            assert_eq!(search.filtered()[0].url, "CONTRIBUTING.md");
        }

        #[test]
        fn move_down_and_up_clamp_at_the_edges() {
            let mut search = MarkdownLinkSearchState::new(links());
            search.move_down();
            assert_eq!(search.selected(), 1);
            search.move_down();
            assert_eq!(search.selected(), 1, "clamped at the last match");
            search.move_up();
            search.move_up();
            assert_eq!(search.selected(), 0, "clamped at the first match");
        }

        #[test]
        fn typing_resets_the_selection_back_to_the_top() {
            let mut search = MarkdownLinkSearchState::new(links());
            search.move_down();
            search.push_char('a');
            assert_eq!(search.selected(), 0);
        }

        #[test]
        fn selected_link_is_none_when_nothing_matches() {
            let mut search = MarkdownLinkSearchState::new(links());
            for c in "nonexistent".chars() {
                search.push_char(c);
            }
            assert_eq!(search.selected_link(), None);
        }

        #[test]
        fn pop_char_removes_the_last_typed_character() {
            let mut search = MarkdownLinkSearchState::new(links());
            search.push_char('x');
            search.push_char('y');
            search.pop_char();
            assert_eq!(search.query(), "x");
        }
    }

    mod handle_markdown_link_search_key_tests {
        use super::*;
        use crate::test_support::{key, test_app};

        fn app_in_search() -> App {
            let dir = unique_scratch_dir("markdown-link-search-keys");
            let path = dir.join("readme.md");
            fs::write(&path, "[Anthropic](https://anthropic.com)\n\n[Contributing](CONTRIBUTING.md)\n").unwrap();
            let mut app = test_app(dir);
            let preview = MarkdownPreviewState::open(&path).unwrap();
            let links = preview.links();
            app.mode = Mode::MarkdownLinkSearch(preview, MarkdownLinkSearchState::new(links));
            app
        }

        #[test]
        fn typing_filters_the_list() {
            let mut app = app_in_search();

            handle_markdown_link_search_key(&mut app, key(KeyCode::Char('a')));

            let Mode::MarkdownLinkSearch(_, search) = &app.mode else { panic!("expected Mode::MarkdownLinkSearch") };
            assert_eq!(search.query(), "a");
        }

        #[test]
        fn esc_cancels_back_to_the_preview_unchanged() {
            let mut app = app_in_search();

            handle_markdown_link_search_key(&mut app, key(KeyCode::Esc));

            assert!(matches!(app.mode, Mode::MarkdownPreview(_)));
        }

        /// The actual end-to-end point of the whole feature: `Enter`
        /// resolves and reacts to the *selected* result, then returns to
        /// the preview with a message recording what happened.
        /// Deliberately selects the *relative, missing-file* link
        /// (`Down` once), not the real absolute URL at index 0 -- a test
        /// that actually opened a real URL would spawn a real OS
        /// process (a real browser) every time this suite runs.
        #[test]
        fn enter_opens_the_selected_link_and_returns_to_the_preview() {
            let mut app = app_in_search();
            handle_markdown_link_search_key(&mut app, key(KeyCode::Down)); // select "Contributing" (CONTRIBUTING.md, doesn't exist here)

            handle_markdown_link_search_key(&mut app, key(KeyCode::Enter));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            let message = state.link_message().expect("should have recorded what Enter did");
            assert!(message.contains("CONTRIBUTING.md"), "message should name the link that was selected: {message:?}");
        }

        #[test]
        fn enter_with_no_matches_just_returns_to_the_preview() {
            let mut app = app_in_search();
            for c in "nonexistent".chars() {
                handle_markdown_link_search_key(&mut app, key(KeyCode::Char(c)));
            }

            handle_markdown_link_search_key(&mut app, key(KeyCode::Enter));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            assert_eq!(state.link_message(), None, "nothing was selected, so nothing should be reported either");
        }

        #[test]
        fn is_a_noop_outside_link_search_mode() {
            let mut app = app_in_search();
            app.mode = Mode::Browsing;

            handle_markdown_link_search_key(&mut app, key(KeyCode::Char('a')));

            assert!(matches!(app.mode, Mode::Browsing));
        }
    }

    mod resolve_link_target_tests {
        use super::*;

        /// The actual point of the whole fix: an in-document anchor
        /// must never be handed to the OS as if it were a real target.
        #[test]
        fn anchor_only_link_has_no_target() {
            let dir = unique_scratch_dir("resolve-link-target");
            assert_eq!(resolve_link_target("#installation", &dir), None);
        }

        #[test]
        fn absolute_http_url_is_returned_as_is() {
            let dir = unique_scratch_dir("resolve-link-target");
            assert_eq!(resolve_link_target("https://example.com", &dir), Some(PathBuf::from("https://example.com")));
        }

        #[test]
        fn mailto_link_is_returned_as_is() {
            let dir = unique_scratch_dir("resolve-link-target");
            assert_eq!(resolve_link_target("mailto:someone@example.com", &dir), Some(PathBuf::from("mailto:someone@example.com")));
        }

        #[test]
        fn relative_link_to_an_existing_file_resolves_against_the_markdown_files_own_directory() {
            let dir = unique_scratch_dir("resolve-link-target");
            fs::write(dir.join("CONTRIBUTING.md"), "hi").unwrap();

            assert_eq!(resolve_link_target("CONTRIBUTING.md", &dir), Some(dir.join("CONTRIBUTING.md")));
        }

        /// Same real bug this whole function exists to prevent: a
        /// relative reference to a file that doesn't actually exist
        /// must not be handed to the OS as a guess either.
        #[test]
        fn relative_link_to_a_missing_file_has_no_target() {
            let dir = unique_scratch_dir("resolve-link-target");
            assert_eq!(resolve_link_target("does-not-exist.md", &dir), None);
        }

        #[test]
        fn relative_link_with_a_fragment_strips_it_before_resolving() {
            let dir = unique_scratch_dir("resolve-link-target");
            fs::write(dir.join("readme.md"), "hi").unwrap();

            assert_eq!(resolve_link_target("readme.md#section", &dir), Some(dir.join("readme.md")));
        }
    }

    mod handle_markdown_preview_mouse_tests {
        use super::*;
        use crate::test_support::test_app;

        /// A left click with no `Ctrl` (or `Ctrl`+click landing on a
        /// line with no link) must never call `system_open::open` --
        /// these tests would spawn a *real* OS process (a real browser)
        /// if that guard were ever removed, so they deliberately stick
        /// to cases with no link to actually open.
        fn app_in_preview() -> App {
            let dir = unique_scratch_dir("markdown-preview-mouse");
            let path = dir.join("readme.md");
            fs::write(&path, "no link here\n\n[a link](https://example.com)\n").unwrap();
            let mut app = test_app(dir);
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(0, 0, 80, 24);
            app.mode = Mode::MarkdownPreview(state);
            app
        }

        fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
            MouseEvent { kind, column, row, modifiers }
        }

        #[test]
        fn plain_click_without_ctrl_does_not_scroll_or_panic() {
            let mut app = app_in_preview();

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::NONE));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            assert_eq!(state.scroll(), 0);
        }

        #[test]
        fn ctrl_click_on_a_line_with_no_link_does_nothing_observable() {
            let mut app = app_in_preview();

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

            assert!(matches!(app.mode, Mode::MarkdownPreview(_)), "should not have crashed or changed mode");
        }

        /// Regression coverage for the real bug: an in-document anchor
        /// link must not be handed to `system_open::open` at all (which
        /// would spawn a real process here if this guard broke) --
        /// `resolve_link_target` already covers the resolution logic in
        /// isolation, this confirms the mouse handler actually calls it
        /// before ever reaching `system_open::open`.
        #[test]
        fn ctrl_click_on_an_anchor_link_does_not_open_anything() {
            let dir = unique_scratch_dir("markdown-preview-mouse");
            let path = dir.join("readme.md");
            fs::write(&path, "[Jump](#section)\n").unwrap();
            let mut app = test_app(dir);
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(0, 0, 80, 24);
            app.mode = Mode::MarkdownPreview(state);

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

            assert!(matches!(app.mode, Mode::MarkdownPreview(_)), "should not have crashed or changed mode");
        }

        /// The actual point of the whole feedback feature: a click that
        /// resolves to nothing openable still leaves a visible trace
        /// (`link_message`), not silence indistinguishable from the
        /// click never having registered at all -- reported directly
        /// after the anchor/missing-file guard above made exactly that
        /// silence the norm.
        #[test]
        fn ctrl_click_on_an_anchor_link_sets_an_explanatory_message() {
            let dir = unique_scratch_dir("markdown-preview-mouse");
            let path = dir.join("readme.md");
            fs::write(&path, "[Jump](#section)\n").unwrap();
            let mut app = test_app(dir);
            let mut state = MarkdownPreviewState::open(&path).unwrap();
            state.set_content_area(0, 0, 80, 24);
            app.mode = Mode::MarkdownPreview(state);

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            let message = state.link_message().expect("should have set a message explaining the click's outcome");
            assert!(message.contains("#section"), "message should name the link that was clicked: {message:?}");
        }

        #[test]
        fn ctrl_click_on_a_line_with_no_link_sets_a_no_link_message() {
            let mut app = app_in_preview();

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            assert!(state.link_message().is_some(), "should say something, not stay silent");
        }

        #[test]
        fn scroll_down_advances_without_needing_ctrl() {
            let mut app = app_in_preview();

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown, 0, 0, KeyModifiers::NONE));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            assert_eq!(state.scroll(), 1);
        }

        #[test]
        fn scroll_up_stops_at_zero() {
            let mut app = app_in_preview();

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp, 0, 0, KeyModifiers::NONE));

            let Mode::MarkdownPreview(state) = &app.mode else { panic!("expected Mode::MarkdownPreview") };
            assert_eq!(state.scroll(), 0);
        }

        #[test]
        fn is_a_noop_outside_markdown_preview_mode() {
            let mut app = app_in_preview();
            app.mode = Mode::Browsing;

            handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown, 0, 0, KeyModifiers::NONE));

            assert!(matches!(app.mode, Mode::Browsing));
        }
    }
}
