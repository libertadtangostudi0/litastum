use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::{MarkdownLine, MarkdownSpan, MarkdownSpanKind};

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
pub(super) fn render_markdown(content: &str) -> Vec<MarkdownLine> {
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
