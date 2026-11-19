use super::*;
#[test]
fn accepts_relative_links_and_blocks_scripts_and_non_web_schemes() {
    assert_eq!(
        link_destination(r"P:\notes\readme.md", "another%20note.md#head"),
        Some(Link::File(r"P:\notes\another note.md".into()))
    );
    assert_eq!(link_destination("/readme.md", "javascript:alert(1)"), None);
    assert_eq!(link_destination("/readme.md", "file:///etc/passwd"), None);
}
#[test]
fn removes_only_a_complete_frontmatter_block() {
    assert_eq!(
        without_frontmatter("\u{feff}---\r\nvalue: x\r\n---\r\n# Note"),
        "# Note"
    );
    assert_eq!(
        without_frontmatter("---\nNo closing mark"),
        "---\nNo closing mark"
    );
}
