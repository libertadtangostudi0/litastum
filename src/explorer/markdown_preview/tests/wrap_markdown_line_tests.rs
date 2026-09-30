use super::*;

fn plain_line(text: &str) -> MarkdownLine {
    vec![MarkdownSpan { text: text.to_string(), kind: MarkdownSpanKind::Plain, url: None }]
}

fn row_texts(rows: &[MarkdownLine]) -> Vec<String> {
    rows.iter().map(|row| row.iter().map(|span| span.text.as_str()).collect()).collect()
}

#[test]
fn a_short_line_stays_on_one_row() {
    let rows = wrap_markdown_line(&plain_line("hello world"), 20);
    assert_eq!(row_texts(&rows), vec!["hello world"]);
}

#[test]
fn wraps_at_a_word_boundary() {
    let rows = wrap_markdown_line(&plain_line("hello world"), 5);
    assert_eq!(row_texts(&rows), vec!["hello", "world"]);
}

/// The actual real-world case this whole fix is for: a link
/// sitting right after a long paragraph that itself wraps into
/// several rows.
#[test]
fn a_link_after_a_wrapped_paragraph_stays_a_link_on_its_own_wrapped_row() {
    let line: MarkdownLine = vec![
        MarkdownSpan { text: "a very long sentence that will definitely need wrapping and then some more ".to_string(), kind: MarkdownSpanKind::Plain, url: None },
        MarkdownSpan { text: "click here".to_string(), kind: MarkdownSpanKind::Link, url: Some("https://example.com".to_string()) },
    ];
    let rows = wrap_markdown_line(&line, 20);
    assert!(rows.len() > 1, "the paragraph should have actually wrapped: {rows:?}");
    let link_row = rows.iter().find(|row| row.iter().any(|span| span.kind == MarkdownSpanKind::Link)).expect("a wrapped row should still carry the link");
    assert_eq!(link_row.iter().find(|s| s.kind == MarkdownSpanKind::Link).unwrap().url.as_deref(), Some("https://example.com"));
}

#[test]
fn a_single_word_longer_than_width_is_hard_split() {
    let rows = wrap_markdown_line(&plain_line("abcdefgh"), 3);
    assert_eq!(row_texts(&rows), vec!["abc", "def", "gh"]);
}

#[test]
fn an_empty_line_stays_a_single_empty_row() {
    let rows = wrap_markdown_line(&Vec::new(), 20);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_empty());
}

#[test]
fn a_link_spanning_a_wrap_point_keeps_its_url_on_both_halves() {
    let line: MarkdownLine = vec![MarkdownSpan { text: "helloworld".to_string(), kind: MarkdownSpanKind::Link, url: Some("https://x.test".to_string()) }];
    let rows = wrap_markdown_line(&line, 5);
    assert_eq!(row_texts(&rows), vec!["hello", "world"]);
    for row in &rows {
        assert_eq!(row[0].url.as_deref(), Some("https://x.test"));
    }
}
