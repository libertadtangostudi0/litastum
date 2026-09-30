use super::*;

fn line_text(line: &MarkdownLine) -> String {
    line.iter().map(|span| span.text.as_str()).collect()
}

#[test]
fn renders_a_heading_with_its_own_level() {
    let (lines, _) = render_markdown("# Title\n");
    let heading = lines.iter().find(|line| !line.is_empty()).unwrap();
    assert_eq!(line_text(heading), "Title");
    assert_eq!(heading[0].kind, MarkdownSpanKind::Heading(1));
}

#[test]
fn renders_bold_and_italic_spans() {
    let (lines, _) = render_markdown("plain **bold** and *italic*\n");
    let line = &lines[0];
    let bold = line.iter().find(|s| s.text == "bold").unwrap();
    assert_eq!(bold.kind, MarkdownSpanKind::Bold);
    let italic = line.iter().find(|s| s.text == "italic").unwrap();
    assert_eq!(italic.kind, MarkdownSpanKind::Italic);
}

#[test]
fn renders_inline_code_as_a_code_span() {
    let (lines, _) = render_markdown("run `cargo test` now\n");
    let code = lines[0].iter().find(|s| s.text == "cargo test").unwrap();
    assert_eq!(code.kind, MarkdownSpanKind::Code);
}

#[test]
fn renders_a_fenced_code_block_as_code_lines() {
    let (lines, _) = render_markdown("```\nfn main() {}\nlet x = 1;\n```\n");
    let code_lines: Vec<&MarkdownLine> = lines.iter().filter(|line| line.iter().any(|s| s.kind == MarkdownSpanKind::Code)).collect();
    assert_eq!(code_lines.len(), 2);
    assert_eq!(line_text(code_lines[0]), "fn main() {}");
    assert_eq!(line_text(code_lines[1]), "let x = 1;");
}

#[test]
fn renders_unordered_list_items_with_a_bullet_prefix() {
    let (lines, _) = render_markdown("- one\n- two\n");
    let items: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(line_text).collect();
    assert_eq!(items, vec!["- one", "- two"]);
}

#[test]
fn renders_ordered_list_items_with_their_own_numbers() {
    let (lines, _) = render_markdown("1. first\n2. second\n");
    let items: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(line_text).collect();
    assert_eq!(items, vec!["1. first", "2. second"]);
}

#[test]
fn renders_nested_list_items_indented() {
    let (lines, _) = render_markdown("- top\n  - nested\n");
    let items: Vec<String> = lines.iter().filter(|l| !l.is_empty()).map(line_text).collect();
    assert_eq!(items[0], "- top");
    assert!(items[1].starts_with("  - "), "nested item should be indented: {items:?}");
}

#[test]
fn renders_a_blockquote_with_the_quote_kind() {
    let (lines, _) = render_markdown("> quoted text\n");
    let line = lines.iter().find(|l| !l.is_empty()).unwrap();
    assert_eq!(line[0].kind, MarkdownSpanKind::Quote);
}

#[test]
fn renders_a_horizontal_rule_as_its_own_line() {
    let (lines, _) = render_markdown("above\n\n---\n\nbelow\n");
    assert!(lines.iter().any(|l| l.len() == 1 && l[0].kind == MarkdownSpanKind::Rule));
}

#[test]
fn trims_trailing_blank_lines() {
    let (lines, _) = render_markdown("one paragraph\n");
    assert!(!lines.is_empty());
    assert!(!lines.last().unwrap().is_empty(), "should not end on a blank line");
}

/// The whole point of returning source rows alongside the rendered
/// lines: `MarkdownPreviewState::sync_to_editor_cursor` needs to
/// know which *source* line each rendered paragraph actually came
/// from, not just its position in the output.
#[test]
fn tracks_the_source_line_each_rendered_paragraph_started_at() {
    let (lines, rows) = render_markdown("first\n\nsecond\n\nthird\n");
    assert_eq!(lines.len(), rows.len(), "rows must stay parallel to lines");
    let content: Vec<(String, usize)> = lines.iter().zip(rows.iter()).filter(|(line, _)| !line.is_empty()).map(|(line, &row)| (line_text(line), row)).collect();
    assert_eq!(content, vec![("first".to_string(), 0), ("second".to_string(), 2), ("third".to_string(), 4)]);
}
