use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use crate::explorer::system_open;

use super::state::MarkdownPreviewState;

/// One link found in the document -- `label` is its own rendered text
/// (what `MarkdownLinkSearchState` searches against, alongside `url`
/// itself), `url` its raw target as written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    pub label: String,
    pub url: String,
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


/// What a Markdown link's raw `url` actually resolves to, once
/// `resolve_link_target` has validated it -- kept as two distinct
/// variants (rather than always a `PathBuf`, an earlier version's
/// approach) so `open_link` can hand a real URL to
/// `system_open::open_url` (`cmd /C start`, no Explorer IPC hop) and a
/// local file to `system_open::open` (`explorer.exe`) -- the two need
/// genuinely different OS commands, see `system_open::build_open_url_command`'s
/// own doc comment for why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinkTarget {
    Url(String),
    File(PathBuf),
}

/// Resolves a Markdown link's raw `url` (as written in the source file)
/// into something actually safe to hand to `system_open` -- `None` for
/// anything that isn't, rather than passing it through blindly.
/// Reported directly as a real bug: a raw in-document anchor
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
///   `LinkTarget::Url`, handed to `system_open::open_url`.
/// - Anything else is treated as a relative reference to another file
///   in the same project, resolved against `markdown_dir` (the
///   previewed file's own directory) -- a leading `#fragment` on such a
///   link (`file.md#section`) is stripped first, then `None` unless the
///   resulting path actually exists on disk, so a broken or
///   not-yet-supported reference never gets handed to the OS as a
///   guess.
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

/// The label to show in `state.link_message()` for `url` -- the text a
/// user actually reads (e.g. "before you start"), never the raw URL
/// itself. Looked up from `state.links()` (first match; a document can
/// in principle link the same target twice under different text, and
/// either label is a fine description of what got opened) rather than
/// threaded through as a separate parameter by every caller -- see
/// `MarkdownPreviewState::link_message`'s own field doc comment for the
/// real bug this replaces (a truncated raw URL that still looked like a
/// complete, valid one).
fn label_for_url(state: &MarkdownPreviewState, url: &str) -> String {
    state.links().into_iter().find(|link| link.url == url).map(|link| link.label).unwrap_or_else(|| url.to_string())
}

/// Resolves and opens `url` (a link's raw target, from either
/// `Ctrl`+click or `Mode::MarkdownLinkSearch`'s own `Enter`), and always
/// leaves a visible record of what happened on `state`
/// (`set_link_message`) -- "Opened: ...", "Failed to open ...: ...", or
/// "Can't open yet: ..." for anything `resolve_link_target` won't vouch
/// for. Requested directly after a first version of `Ctrl`+click left no
/// visible trace at all once it started correctly refusing to open
/// anchors/missing files -- indistinguishable from the click not
/// registering. The message itself names the link's *label*
/// (`label_for_url`), not its raw URL -- see `link_message`'s own field
/// doc comment for why.
pub(super) fn open_link(state: &mut MarkdownPreviewState, url: &str) {
    let markdown_dir = state.path().parent().map(Path::to_path_buf).unwrap_or_default();
    let label = label_for_url(state, url);
    match resolve_link_target(url, &markdown_dir) {
        Some(LinkTarget::Url(target)) => {
            debug!(url, "markdown preview: opening link as a url");
            match system_open::open_url(&target) {
                Ok(_) => state.set_link_message(format!("Opened: {label}")),
                Err(err) => {
                    warn!(url, %err, "failed to open a markdown preview link as a url");
                    state.set_link_message(format!("Failed to open {label}: {err}"));
                }
            }
        }
        Some(LinkTarget::File(target)) => {
            debug!(url, target = %target.display(), "markdown preview: opening link as a file");
            match system_open::open(&target) {
                Ok(_) => state.set_link_message(format!("Opened: {label}")),
                Err(err) => {
                    warn!(url, target = %target.display(), %err, "failed to open a markdown preview link");
                    state.set_link_message(format!("Failed to open {label}: {err}"));
                }
            }
        }
        None => {
            debug!(url, "markdown preview: link has no openable target (anchor, or relative file not found)");
            state.set_link_message(format!("Can't open yet: {label} (in-document anchor, or file not found)"));
        }
    }
}
