use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::{MarkdownLine, MarkdownSpan, MarkdownSpanKind};

/// Turns `content` into styled lines -- the common constructs only:
/// headings, emphasis, inline/fenced code, quotes, nested lists, links
/// (label shown, URL kept on the span for clicks), rules. Tables,
/// footnotes, task boxes and images are dropped, not misrendered. Loose
/// lists get an extra blank line between items (known, minor).
///
/// Also returns each line's source line (from `into_offset_iter`) for
/// `sync_to_editor_cursor` -- approximate: a soft-wrapped paragraph gets
/// its first line, a blank separator the line its block closed on.
pub(super) fn render_markdown(content: &str) -> (Vec<MarkdownLine>, Vec<usize>) {
    let mut lines: Vec<MarkdownLine> = Vec::new();
    let mut line_rows: Vec<usize> = Vec::new();
    let mut current: MarkdownLine = Vec::new();
    let mut current_row: Option<usize> = None;

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

    let flush = |current: &mut MarkdownLine, lines: &mut Vec<MarkdownLine>, current_row: &mut Option<usize>, line_rows: &mut Vec<usize>, fallback_row: usize| {
        if !current.is_empty() {
            lines.push(std::mem::take(current));
            line_rows.push(current_row.take().unwrap_or(fallback_row));
        }
        *current_row = None;
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

    for (event, range) in Parser::new(content).into_offset_iter() {
        let row = line_at_offset(content, range.start);
        match event {
            Event::End(TagEnd::Paragraph) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                lines.push(Vec::new());
                line_rows.push(row);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                heading = Some(heading_level_number(level));
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                heading = None;
                lines.push(Vec::new());
                line_rows.push(row);
            }
            Event::Start(Tag::Strong) => bold_depth += 1,
            Event::End(TagEnd::Strong) => bold_depth = bold_depth.saturating_sub(1),
            Event::Start(Tag::Emphasis) => italic_depth += 1,
            Event::End(TagEnd::Emphasis) => italic_depth = italic_depth.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => link_url = Some(dest_url.into_string()),
            Event::End(TagEnd::Link) => link_url = None,
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                quote_depth = quote_depth.saturating_sub(1);
                lines.push(Vec::new());
                line_rows.push(row);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                lines.push(Vec::new());
                line_rows.push(row);
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                list_stack.push((start.is_some(), start.unwrap_or(1)));
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
                lines.push(Vec::new());
                line_rows.push(row);
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                let prefix = match list_stack.last_mut() {
                    Some((true, counter)) => {
                        let n = *counter;
                        *counter += 1;
                        format!("{indent}{n}. ")
                    }
                    _ => format!("{indent}- "),
                };
                current_row.get_or_insert(row);
                current.push(MarkdownSpan { text: prefix, kind: MarkdownSpanKind::Plain, url: None });
            }
            Event::End(TagEnd::Item) => flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row),
            Event::Rule => {
                flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                lines.push(vec![MarkdownSpan { text: "\u{2500}".repeat(40), kind: MarkdownSpanKind::Rule, url: None }]);
                line_rows.push(row);
                lines.push(Vec::new());
                line_rows.push(row);
            }
            Event::Text(text) => {
                if in_code_block {
                    flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row);
                    // A fenced/indented code block's whole body arrives
                    // as one `Text` event with embedded newlines --
                    // `split('\n')` on text ending in `\n` leaves one
                    // spurious empty trailing element, dropped below.
                    let mut code_lines: Vec<&str> = text.split('\n').collect();
                    if text.ends_with('\n') {
                        code_lines.pop();
                    }
                    for (i, code_line) in code_lines.into_iter().enumerate() {
                        lines.push(vec![MarkdownSpan { text: code_line.to_string(), kind: MarkdownSpanKind::Code, url: None }]);
                        line_rows.push(row + i);
                    }
                } else {
                    current_row.get_or_insert(row);
                    let kind = span_kind(heading, bold_depth, italic_depth, link_url.is_some(), quote_depth);
                    current.push(MarkdownSpan { text: text.into_string(), kind, url: link_url.clone() });
                }
            }
            Event::Code(text) => {
                current_row.get_or_insert(row);
                current.push(MarkdownSpan { text: text.into_string(), kind: MarkdownSpanKind::Code, url: None });
            }
            Event::SoftBreak => {
                current_row.get_or_insert(row);
                current.push(MarkdownSpan { text: " ".to_string(), kind: MarkdownSpanKind::Plain, url: None });
            }
            Event::HardBreak => flush(&mut current, &mut lines, &mut current_row, &mut line_rows, row),
            _ => {}
        }
    }
    let end_row = line_at_offset(content, content.len());
    flush(&mut current, &mut lines, &mut current_row, &mut line_rows, end_row);

    // A tidier ending than however many blank "paragraph/heading/list
    // just closed" lines happened to accumulate at the very end.
    while matches!(lines.last(), Some(line) if line.is_empty()) {
        lines.pop();
        line_rows.pop();
    }

    (lines, line_rows)
}

/// The 0-indexed source line containing byte `offset` into `content` --
/// a plain newline count, since `pulldown_cmark`'s own byte ranges are
/// always at valid UTF-8 (and therefore valid line-counting) boundaries.
fn line_at_offset(content: &str, offset: usize) -> usize {
    content.as_bytes()[..offset.min(content.len())].iter().filter(|&&b| b == b'\n').count()
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
