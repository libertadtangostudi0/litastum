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
