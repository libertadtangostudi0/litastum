use std::fs;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::{App, Mode};
use crate::test_support::unique_scratch_dir;

use super::links::{resolve_link_target, LinkTarget};
use super::render::render_markdown;
use super::*;

mod wrap_markdown_line_tests {
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
}

mod render_markdown_tests {
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

    #[test]
    fn page_down_advances_by_a_page_and_clamps_at_the_last_line() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        // One rendered line per paragraph -- enough blank-line-separated
        // paragraphs that a single PAGE_SIZE (15) page_down doesn't
        // already reach the end, so this actually exercises the "advance
        // by a page" arithmetic and not just the clamp.
        let content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
        fs::write(&path, content).unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        let last = state.lines().len() - 1;
        assert!(last > 15, "fixture should be taller than one page");

        state.page_down();
        assert_eq!(state.scroll(), 15, "should advance by exactly one page");

        for _ in 0..10 {
            state.page_down();
        }
        assert_eq!(state.scroll(), last, "repeated page_down should clamp at the last line, not overshoot");
    }

    #[test]
    fn page_up_retreats_by_a_page_and_clamps_at_zero() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        let content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
        fs::write(&path, content).unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        state.page_down();
        state.page_down(); // scroll = 30

        state.page_up();
        assert_eq!(state.scroll(), 15, "should retreat by exactly one page");

        state.page_up();
        state.page_up();
        assert_eq!(state.scroll(), 0, "repeated page_up should clamp at zero, not underflow");
    }

    /// The actual point of `reload`: re-reading the file after a save
    /// picks up the new content instead of showing stale rendered
    /// lines.
    #[test]
    fn reload_picks_up_the_files_new_content() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "old\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();

        fs::write(&path, "brand new content\n").unwrap();
        state.reload();

        let text: String = state.lines().iter().flatten().map(|span| span.text.as_str()).collect();
        assert!(text.contains("brand new content"), "should reflect the file's new content: {text:?}");
    }

    /// Regression coverage for the actual point of clamping rather than
    /// resetting to `0` on reload: editing near the end of a long
    /// document and saving shouldn't jump the preview back to the top,
    /// but a scroll position past a now-*shorter* document has to be
    /// pulled back in bounds rather than left pointing past the end.
    #[test]
    fn reload_clamps_the_scroll_position_to_the_new_shorter_content() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        let long_content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
        fs::write(&path, &long_content).unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        for _ in 0..40 {
            state.scroll_down();
        }
        let scroll_before = state.scroll();
        assert!(scroll_before > 2, "sanity: scrolled well past a 3-line document");

        fs::write(&path, "a\n\nb\n\nc\n").unwrap(); // much shorter now
        state.reload();

        assert_eq!(state.scroll(), state.lines().len() - 1, "scroll should be pulled back in bounds, not left pointing past the new, shorter content");
    }

    /// `reload` on a file that's vanished (or otherwise fails to read)
    /// must leave the previously-good preview untouched rather than
    /// blanking it -- same "never blocks on this" convention the rest
    /// of this module follows.
    #[test]
    fn reload_leaves_the_preview_untouched_on_a_read_failure() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "still here\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        let lines_before = state.lines().len();

        fs::remove_file(&path).unwrap();
        state.reload();

        assert_eq!(state.lines().len(), lines_before, "should keep showing the last-good content, not blank out");
    }

    /// The actual point of the whole click-a-link feature: a click
    /// landing on the rendered row holding the link finds its URL.
    /// `visible_row_links` is populated by hand here, matching what
    /// `ui::markdown_preview::draw_markdown_preview` would actually
    /// build for this content -- `link_at` itself only ever reads that
    /// table, it doesn't re-derive it from `state.lines()`.
    #[test]
    fn link_at_finds_the_url_on_the_clicked_line() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        state.set_content_area(0, 0, 80, 24);
        state.set_visible_row_links(vec![vec![(0, "Anthropic".chars().count() as u16, "https://anthropic.com".to_string())]]);

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

    /// Scrolling changes which logical line lands on visual row 0 --
    /// `draw_markdown_preview` re-wraps and rebuilds `visible_row_links`
    /// from `state.scroll()` onward every frame, so once scrolled past
    /// "line one" and the blank line, row 0's own hitbox table is the
    /// link line's, exactly as set here.
    #[test]
    fn link_at_accounts_for_scroll_offset() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "line one\n\n[link](https://example.com)\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        state.set_content_area(0, 0, 80, 24);
        state.scroll_down();
        state.scroll_down();
        state.set_visible_row_links(vec![vec![(0, "link".chars().count() as u16, "https://example.com".to_string())]]);

        assert_eq!(state.link_at(0, 0), Some("https://example.com"), "row 0 on screen should now be the scrolled-to line holding the link");
    }

    fn line_text_at(state: &MarkdownPreviewState, index: usize) -> String {
        state.lines()[index].iter().map(|span| span.text.as_str()).collect()
    }

    /// The actual point of the whole sync feature: moving the editor's
    /// cursor onto a source line highlights the rendered line it became
    /// and scrolls it into view.
    #[test]
    fn sync_to_editor_cursor_highlights_and_scrolls_to_the_matching_line() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "first\n\nsecond\n\nthird\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();

        state.sync_to_editor_cursor(2, 0.0, 24); // "second" starts at source line 2

        let index = state.highlighted_line().expect("should have matched a line");
        assert_eq!(line_text_at(&state, index), "second");
        assert_eq!(state.scroll(), index);
    }

    /// The actual point of the follow-up request: a cursor positioned
    /// halfway down the *editor's* own visible page should scroll the
    /// preview so the matched line lands halfway down *its* own page
    /// too, not always snapped to the top.
    #[test]
    fn sync_to_editor_cursor_offsets_the_scroll_by_the_relative_position() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        let content: String = (0..20).map(|i| format!("line {i}\n\n")).collect();
        fs::write(&path, &content).unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();

        // "line 10" starts at source row 20 (each paragraph plus its
        // own trailing blank separator takes 2 source lines).
        state.sync_to_editor_cursor(20, 0.5, 10);

        let index = state.highlighted_line().expect("should have matched a line");
        assert_eq!(line_text_at(&state, index), "line 10");
        assert_eq!(state.scroll(), index - 5, "should scroll back by half the given visible height");
    }

    /// A cursor row with no rendered line of its own (a blank separator
    /// line between two paragraphs) should still highlight *something*
    /// sensible -- the closest preceding real content, not nothing and
    /// not the next paragraph down.
    #[test]
    fn sync_to_editor_cursor_falls_back_to_the_closest_preceding_line() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "first\n\nsecond\n").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();

        state.sync_to_editor_cursor(1, 0.0, 24); // the blank line between "first" and "second"

        let index = state.highlighted_line().unwrap();
        assert_eq!(line_text_at(&state, index), "first");
    }

    #[test]
    fn sync_to_editor_cursor_is_a_noop_on_an_empty_document() {
        let dir = unique_scratch_dir("markdown-preview");
        let path = dir.join("readme.md");
        fs::write(&path, "").unwrap();
        let mut state = MarkdownPreviewState::open(&path).unwrap();

        state.sync_to_editor_cursor(0, 0.0, 24);

        assert_eq!(state.highlighted_line(), None);
    }
}

mod open_edit_preview_tests {
    use super::*;
    use crate::test_support::test_app;

    #[test]
    fn opens_the_editor_and_a_linked_preview_with_the_editor_active() {
        let dir = unique_scratch_dir("markdown-preview-open");
        fs::write(dir.join("readme.md"), "# hi\n").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "readme.md").unwrap();

        open_edit_preview(&mut app);

        assert!(matches!(app.mode, Mode::Editing(_)));
        assert!(app.markdown_edit_preview.is_some(), "should link a live preview alongside the editor");
        assert_eq!(app.active, 0, "the left panel (the editor) should start active, ready to type into");
    }

    #[test]
    fn is_a_noop_on_a_non_markdown_file() {
        let dir = unique_scratch_dir("markdown-preview-open");
        fs::write(dir.join("notes.txt"), "hi").unwrap();
        let mut app = test_app(dir);
        app.panels[0].selected = app.panels[0].entries.iter().position(|e| e.name == "notes.txt").unwrap();

        open_edit_preview(&mut app);

        assert!(matches!(app.mode, Mode::Browsing));
        assert!(app.markdown_edit_preview.is_none());
    }
}

mod handle_markdown_edit_preview_key_tests {
    use super::*;
    use crate::editor::Editor;
    use crate::test_support::{key, test_app};

    /// Builds a combined editor+preview session (`App::markdown_edit_preview`
    /// linked to `Mode::Editing`), focused on the preview half
    /// (`app.active = 1`) -- the state `main.rs::handle_key_event`
    /// routes to `handle_markdown_edit_preview_key` in the first place.
    fn app_in_preview(content: &str) -> App {
        let dir = unique_scratch_dir("markdown-preview-keys");
        let path = dir.join("readme.md");
        fs::write(&path, content).unwrap();
        let mut app = test_app(dir);
        let preview = MarkdownPreviewState::open(&path).unwrap();
        let editor = Editor::open(path, None).unwrap();
        app.markdown_edit_preview = Some(preview);
        app.mode = Mode::Editing(editor);
        app.active = 1;
        app
    }

    #[test]
    fn down_scrolls_the_preview_forward() {
        let mut app = app_in_preview("line one\n\nline two\n\nline three\n");

        handle_markdown_edit_preview_key(&mut app, key(KeyCode::Down)).unwrap();

        assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 1);
    }

    /// `Esc` closes the *whole* editor+preview session (`editor::close_editor_or_confirm`),
    /// not just the preview half -- an unmodified, freshly opened editor
    /// has no unsaved changes, so this returns straight to `Mode::Browsing`
    /// with the linked preview cleared.
    #[test]
    fn esc_closes_the_whole_session() {
        let mut app = app_in_preview("hello\n");

        handle_markdown_edit_preview_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert!(app.markdown_edit_preview.is_none(), "closing should drop the linked preview too");
    }

    #[test]
    fn f3_again_also_closes_the_whole_session() {
        let mut app = app_in_preview("hello\n");

        handle_markdown_edit_preview_key(&mut app, key(KeyCode::F(3))).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn is_a_noop_without_a_linked_preview() {
        let dir = unique_scratch_dir("markdown-preview-keys");
        let path = dir.join("readme.md");
        fs::write(&path, "hello\n").unwrap();
        let mut app = test_app(dir);
        app.mode = Mode::Editing(Editor::open(path, None).unwrap());

        handle_markdown_edit_preview_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(app.markdown_edit_preview.is_none());
    }

    /// The actual point of the whole keyboard-search feature: `l` opens
    /// it, parking the editor so `Esc`/`Enter` can hand it straight
    /// back.
    #[test]
    fn l_opens_the_link_search_when_the_document_has_links() {
        let mut app = app_in_preview("[Anthropic](https://anthropic.com)\n");

        handle_markdown_edit_preview_key(&mut app, key(KeyCode::Char('l'))).unwrap();

        assert!(matches!(app.mode, Mode::MarkdownLinkSearch(..)));
    }

    #[test]
    fn l_is_a_noop_when_the_document_has_no_links() {
        let mut app = app_in_preview("no links here\n");

        handle_markdown_edit_preview_key(&mut app, key(KeyCode::Char('l'))).unwrap();

        assert!(matches!(app.mode, Mode::Editing(_)), "should stay put, nothing to search");
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
    use crate::editor::Editor;
    use crate::test_support::{key, test_app};

    /// `Mode::MarkdownLinkSearch` "parks" the `Editor` in its own tuple
    /// (see its own doc comment) -- `App::markdown_edit_preview` is
    /// untouched throughout, exactly as it is during ordinary
    /// `Mode::Editing`.
    fn app_in_search() -> App {
        let dir = unique_scratch_dir("markdown-link-search-keys");
        let path = dir.join("readme.md");
        fs::write(&path, "[Anthropic](https://anthropic.com)\n\n[Contributing](CONTRIBUTING.md)\n").unwrap();
        let mut app = test_app(dir);
        let preview = MarkdownPreviewState::open(&path).unwrap();
        let links = preview.links();
        let editor = Editor::open(path, None).unwrap();
        app.markdown_edit_preview = Some(preview);
        app.mode = Mode::MarkdownLinkSearch(editor, MarkdownLinkSearchState::new(links));
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
    fn esc_cancels_back_to_editing_with_the_preview_still_linked() {
        let mut app = app_in_search();

        handle_markdown_link_search_key(&mut app, key(KeyCode::Esc));

        assert!(matches!(app.mode, Mode::Editing(_)));
        assert!(app.markdown_edit_preview.is_some());
    }

    /// The actual end-to-end point of the whole feature: `Enter`
    /// resolves and reacts to the *selected* result, then returns to
    /// editing with a message recording what happened on the linked
    /// preview -- naming the link's *label* ("Contributing"), not its
    /// raw URL, per `MarkdownPreviewState::link_message`'s own field
    /// doc comment. Deliberately selects the *relative, missing-file*
    /// link (`Down` once), not the real absolute URL at index 0 -- a
    /// test that actually opened a real URL would spawn a real OS
    /// process (a real browser) every time this suite runs.
    #[test]
    fn enter_opens_the_selected_link_and_returns_to_editing() {
        let mut app = app_in_search();
        handle_markdown_link_search_key(&mut app, key(KeyCode::Down)); // select "Contributing" (CONTRIBUTING.md, doesn't exist here)

        handle_markdown_link_search_key(&mut app, key(KeyCode::Enter));

        assert!(matches!(app.mode, Mode::Editing(_)));
        let message = app.markdown_edit_preview.as_ref().unwrap().link_message().expect("should have recorded what Enter did");
        assert!(message.contains("Contributing"), "message should name the label of the link that was selected: {message:?}");
    }

    #[test]
    fn enter_with_no_matches_just_returns_to_editing() {
        let mut app = app_in_search();
        for c in "nonexistent".chars() {
            handle_markdown_link_search_key(&mut app, key(KeyCode::Char(c)));
        }

        handle_markdown_link_search_key(&mut app, key(KeyCode::Enter));

        assert!(matches!(app.mode, Mode::Editing(_)));
        assert_eq!(app.markdown_edit_preview.as_ref().unwrap().link_message(), None, "nothing was selected, so nothing should be reported either");
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

    /// The actual point of the whole fix: an in-document anchor must
    /// never be handed to the OS as if it were a real target.
    #[test]
    fn anchor_only_link_has_no_target() {
        let dir = unique_scratch_dir("resolve-link-target");
        assert_eq!(resolve_link_target("#installation", &dir), None);
    }

    #[test]
    fn absolute_http_url_is_returned_as_a_url_target() {
        let dir = unique_scratch_dir("resolve-link-target");
        assert_eq!(resolve_link_target("https://example.com", &dir), Some(LinkTarget::Url("https://example.com".to_string())));
    }

    #[test]
    fn mailto_link_is_returned_as_a_url_target() {
        let dir = unique_scratch_dir("resolve-link-target");
        assert_eq!(resolve_link_target("mailto:someone@example.com", &dir), Some(LinkTarget::Url("mailto:someone@example.com".to_string())));
    }

    #[test]
    fn relative_link_to_an_existing_file_resolves_against_the_markdown_files_own_directory() {
        let dir = unique_scratch_dir("resolve-link-target");
        fs::write(dir.join("CONTRIBUTING.md"), "hi").unwrap();

        assert_eq!(resolve_link_target("CONTRIBUTING.md", &dir), Some(LinkTarget::File(dir.join("CONTRIBUTING.md"))));
    }

    /// Same real bug this whole function exists to prevent: a relative
    /// reference to a file that doesn't actually exist must not be
    /// handed to the OS as a guess either.
    #[test]
    fn relative_link_to_a_missing_file_has_no_target() {
        let dir = unique_scratch_dir("resolve-link-target");
        assert_eq!(resolve_link_target("does-not-exist.md", &dir), None);
    }

    #[test]
    fn relative_link_with_a_fragment_strips_it_before_resolving() {
        let dir = unique_scratch_dir("resolve-link-target");
        fs::write(dir.join("readme.md"), "hi").unwrap();

        assert_eq!(resolve_link_target("readme.md#section", &dir), Some(LinkTarget::File(dir.join("readme.md"))));
    }
}

mod handle_markdown_preview_mouse_tests {
    use super::*;
    use crate::editor::Editor;
    use crate::test_support::test_app;

    /// A left click with no `Ctrl` (or `Ctrl`+click landing on a line
    /// with no link) must never call `system_open::open` -- these tests
    /// would spawn a *real* OS process (a real browser) if that guard
    /// were ever removed, so they deliberately stick to cases with no
    /// link to actually open.
    fn app_in_preview() -> App {
        let dir = unique_scratch_dir("markdown-preview-mouse");
        let path = dir.join("readme.md");
        fs::write(&path, "no link here\n\n[a link](https://example.com)\n").unwrap();
        let mut app = test_app(dir);
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        state.set_content_area(0, 0, 80, 24);
        let editor = Editor::open(path, None).unwrap();
        app.markdown_edit_preview = Some(state);
        app.mode = Mode::Editing(editor);
        app
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent { kind, column, row, modifiers }
    }

    #[test]
    fn plain_click_without_ctrl_does_not_scroll_or_panic() {
        let mut app = app_in_preview();

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::NONE));

        assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 0);
    }

    #[test]
    fn ctrl_click_on_a_line_with_no_link_does_nothing_observable() {
        let mut app = app_in_preview();

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

        assert!(matches!(app.mode, Mode::Editing(_)), "should not have crashed or changed mode");
    }

    /// Regression coverage for the real bug: an in-document anchor link
    /// must not be handed to `system_open::open` at all (which would
    /// spawn a real process here if this guard broke) --
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
        state.set_visible_row_links(vec![vec![(0, "Jump".chars().count() as u16, "#section".to_string())]]);
        let editor = Editor::open(path, None).unwrap();
        app.markdown_edit_preview = Some(state);
        app.mode = Mode::Editing(editor);

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

        assert!(matches!(app.mode, Mode::Editing(_)), "should not have crashed or changed mode");
    }

    /// The actual point of the whole feedback feature: a click that
    /// resolves to nothing openable still leaves a visible trace
    /// (`link_message`), not silence indistinguishable from the click
    /// never having registered at all -- reported directly after the
    /// anchor/missing-file guard above made exactly that silence the
    /// norm. Checks for the link's *label* ("Jump"), not its raw URL --
    /// see `MarkdownPreviewState::link_message`'s own field doc comment
    /// for why a raw URL is never embedded here.
    #[test]
    fn ctrl_click_on_an_anchor_link_sets_an_explanatory_message() {
        let dir = unique_scratch_dir("markdown-preview-mouse");
        let path = dir.join("readme.md");
        fs::write(&path, "[Jump](#section)\n").unwrap();
        let mut app = test_app(dir);
        let mut state = MarkdownPreviewState::open(&path).unwrap();
        state.set_content_area(0, 0, 80, 24);
        state.set_visible_row_links(vec![vec![(0, "Jump".chars().count() as u16, "#section".to_string())]]);
        let editor = Editor::open(path, None).unwrap();
        app.markdown_edit_preview = Some(state);
        app.mode = Mode::Editing(editor);

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

        let message = app.markdown_edit_preview.as_ref().unwrap().link_message().expect("should have set a message explaining the click's outcome");
        assert!(message.contains("Jump"), "message should name the label of the link that was clicked: {message:?}");
    }

    #[test]
    fn ctrl_click_on_a_line_with_no_link_sets_a_no_link_message() {
        let mut app = app_in_preview();

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL));

        assert!(app.markdown_edit_preview.as_ref().unwrap().link_message().is_some(), "should say something, not stay silent");
    }

    #[test]
    fn scroll_down_advances_without_needing_ctrl() {
        let mut app = app_in_preview();

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown, 0, 0, KeyModifiers::NONE));

        assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 1);
    }

    #[test]
    fn scroll_up_stops_at_zero() {
        let mut app = app_in_preview();

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp, 0, 0, KeyModifiers::NONE));

        assert_eq!(app.markdown_edit_preview.as_ref().unwrap().scroll(), 0);
    }

    #[test]
    fn is_a_noop_outside_editing_mode() {
        let mut app = app_in_preview();
        app.mode = Mode::Browsing;

        handle_markdown_preview_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown, 0, 0, KeyModifiers::NONE));

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
