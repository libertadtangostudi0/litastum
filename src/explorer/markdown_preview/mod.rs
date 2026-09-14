mod input;
mod links;
mod render;
mod state;
mod wrap;

use std::path::Path;

pub use input::{handle_markdown_edit_preview_key, handle_markdown_link_search_key, handle_markdown_preview_mouse, open_edit_preview};
pub use links::MarkdownLinkSearchState;
pub use state::MarkdownPreviewState;
pub use wrap::wrap_markdown_line;
// `MarkdownLink` has no production caller outside this module tree --
// only `crate::explorer`'s own test-only re-export (for
// `ui::markdown_preview`'s tests) and this module's own `tests.rs`
// need it from outside `links.rs` itself, same reasoning as
// `explorer::Prompt`/`MenuItem`'s own test-only re-export.
#[cfg(test)]
pub use links::MarkdownLink;

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


#[cfg(test)]
mod tests;

#[cfg(test)]
mod is_markdown_file_tests {
    use super::*;

    #[test]
    fn recognizes_md_and_markdown_case_insensitively() {
        assert!(is_markdown_file(Path::new("readme.md")));
        assert!(is_markdown_file(Path::new("README.MD")));
        assert!(is_markdown_file(Path::new("notes.markdown")));
    }

    #[test]
    fn rejects_other_extensions() {
        assert!(!is_markdown_file(Path::new("photo.png")));
        assert!(!is_markdown_file(Path::new("no_extension")));
    }
}
