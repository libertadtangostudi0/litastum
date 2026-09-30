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
