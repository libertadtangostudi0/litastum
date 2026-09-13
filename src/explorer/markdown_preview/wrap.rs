use super::{MarkdownLine, MarkdownSpan};

/// Word-wraps one logical `MarkdownLine` into however many visual rows
/// it takes to fit `width` columns, each returned row a `MarkdownLine`
/// of its own (spans split at wrap points, but each split piece keeps
/// its original `kind`/`url` -- so a link that happens to straddle a
/// wrap point still resolves correctly on either row it lands on).
///
/// `ui::markdown_preview::draw_markdown_preview` calls this instead of
/// handing `ratatui`'s own `Paragraph` a `Wrap` to do this on its own --
/// the point isn't the wrapping itself (either would look the same on
/// screen) but that *this* module can then build exact click hitboxes
/// (`MarkdownPreviewState::set_visible_row_links`) from the very same
/// rows actually rendered, rather than a separate row-to-logical-line
/// guess that has no way to know where `ratatui`'s own wrap points
/// landed.
pub fn wrap_markdown_line(line: &MarkdownLine, width: usize) -> Vec<MarkdownLine> {
    if line.is_empty() {
        return vec![Vec::new()];
    }

    // Flatten to one char sequence, remembering which original span
    // each character came from, so a wrapped row can be reconstructed
    // with the right `kind`/`url` per run of characters.
    let mut chars: Vec<char> = Vec::new();
    let mut span_of_char: Vec<usize> = Vec::new();
    for (span_index, span) in line.iter().enumerate() {
        for ch in span.text.chars() {
            chars.push(ch);
            span_of_char.push(span_index);
        }
    }
    if chars.is_empty() {
        return vec![Vec::new()];
    }

    wrap_ranges(&chars, width)
        .into_iter()
        .map(|(start, end)| {
            let mut row: MarkdownLine = Vec::new();
            let mut i = start;
            while i < end {
                let span_index = span_of_char[i];
                let run_start = i;
                while i < end && span_of_char[i] == span_index {
                    i += 1;
                }
                let text: String = chars[run_start..i].iter().collect();
                row.push(MarkdownSpan { text, kind: line[span_index].kind, url: line[span_index].url.clone() });
            }
            row
        })
        .collect()
}

/// Greedy word-wrap of `chars` into rows of at most `width` columns --
/// returns each row's `(start, end)` character-offset range into
/// `chars`. Wraps at whitespace boundaries, same convention any
/// ordinary word-wrap uses; the separating space at a break point is
/// swallowed (included in neither row). A single word longer than
/// `width` on its own is hard-split into `width`-wide chunks, since
/// there's nowhere else to put it.
pub(super) fn wrap_ranges(chars: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    if chars.is_empty() {
        return vec![(0, 0)];
    }

    let mut rows = Vec::new();
    let mut row_start = 0usize;
    let mut row_len = 0usize;
    let mut cursor = 0usize;

    loop {
        while cursor < chars.len() && chars[cursor] == ' ' {
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }
        let word_start = cursor;
        while cursor < chars.len() && chars[cursor] != ' ' {
            cursor += 1;
        }
        let word_end = cursor;
        let word_len = word_end - word_start;
        let sep = if row_len > 0 { 1 } else { 0 };

        if row_len > 0 && row_len + sep + word_len > width {
            rows.push((row_start, row_start + row_len));
            row_start = word_start;
            row_len = 0;
        }

        if word_len > width {
            if row_len > 0 {
                rows.push((row_start, row_start + row_len));
            }
            let mut chunk_start = word_start;
            while word_end - chunk_start > width {
                rows.push((chunk_start, chunk_start + width));
                chunk_start += width;
            }
            row_start = chunk_start;
            row_len = word_end - chunk_start;
            continue;
        }

        if row_len == 0 {
            row_start = word_start;
            row_len = word_len;
        } else {
            row_len += 1 + word_len;
        }
    }

    rows.push((row_start, row_start + row_len));
    rows
}
