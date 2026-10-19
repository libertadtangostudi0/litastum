use std::path::{Path, PathBuf};

use super::state::MarkdownPreviewState;

/// One link found in the document -- `label` is its own rendered text
/// (what `MarkdownLinkSearchState` searches against, alongside `url`
/// itself), `url` its raw target as written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    pub label: String,
    pub url: String,
}


/// `Overlay::MarkdownLinkSearch`: a filterable list of the document's
/// links. Typing filters labels/URLs case-insensitively, `Up`/`Down` move,
/// `Enter` opens (`open_link`, as for `Ctrl`+click). Append/backspace
/// only -- a short filter needs no full text field.
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


/// What a link resolves to: a URL (`system_open::open_url`) or a local
/// file (`system_open::open`) -- they need different OS commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinkTarget {
    Url(String),
    File(PathBuf),
}

/// Resolves a link's raw `url` into something safe to open, else `None`:
/// - `#fragment` alone -- not supported yet (no heading index);
/// - contains `://` or is `mailto:` -- `LinkTarget::Url`;
/// - anything else -- a file relative to `markdown_dir`, with any
///   `#fragment` stripped, only if it exists.
///
/// Anything unresolved passed to `explorer.exe` opened its default folder
/// instead. History: docs/history/markdown-preview.md.
pub(super) fn resolve_link_target(url: &str, markdown_dir: &Path) -> Option<LinkTarget> {
    if url.starts_with('#') {
        return None;
    }
    if url.contains("://") || url.starts_with("mailto:") {
        return Some(LinkTarget::Url(url.to_string()));
    }

    let relative = url.split('#').next().unwrap_or(url);
    if relative.is_empty() {
        return None;
    }
    let target = markdown_dir.join(relative);
    target.exists().then_some(LinkTarget::File(target))
}

/// The link's label for the status message (first match in
/// `state.links()`), never the raw URL -- see `link_message`.
pub(super) fn label_for_url(state: &MarkdownPreviewState, url: &str) -> String {
    state.links().into_iter().find(|link| link.url == url).map(|link| link.label).unwrap_or_else(|| url.to_string())
}
