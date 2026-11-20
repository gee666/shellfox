use super::*;
#[test]
fn accepts_relative_links_and_blocks_scripts_and_non_web_schemes() {
    assert_eq!(
        link_destination(r"P:\notes\readme.md", "another%20note.md#head"),
        Some(Link::File(r"P:\notes\another note.md".into()))
    );
    assert_eq!(link_destination("/readme.md", "javascript:alert(1)"), None);
    assert_eq!(link_destination("/readme.md", "file:///etc/passwd"), None);
    assert_eq!(link_destination("/readme.md", "https:example.test"), None);
    assert_eq!(
        link_destination("/readme.md", "https://example.test/\n"),
        None
    );
    assert_eq!(
        link_destination("/readme.md", "HTTPS://example.test/page"),
        Some(Link::External("https://example.test/page".into()))
    );
    assert_eq!(link_destination("/readme.md", "%2Fetc/passwd"), None);
    assert_eq!(link_destination("/readme.md", "%5C%5Cserver%5Cshare"), None);
    assert_eq!(
        link_destination("/readme.md", "#mise-%C3%A0-jour"),
        Some(Link::Anchor("mise-à-jour".into()))
    );
    assert_eq!(anchors::slug(" Mise À jour! "), "mise-à-jour");
}
#[test]
fn linked_images_keep_their_outer_link_and_neighbor_styles() {
    let md = Markdown::parse("Before **bold [![map](map.svg)](next.md) tail** after");
    let [Node::Group(Tag::Paragraph, children)] = md.0.as_slice() else {
        panic!("paragraph expected")
    };
    let parts = inline::parts(children, Palette::new("#ec4899", "#111016"), true);
    let [
        inline::Part::Text(before),
        inline::Part::Image { source, alt, link },
        inline::Part::Text(after),
    ] = parts.as_slice()
    else {
        panic!("text/image/text expected")
    };
    assert_eq!(before.text, "Before bold ");
    assert_eq!(after.text, " tail after");
    assert_eq!(source, "map.svg");
    assert_eq!(alt, "map");
    assert_eq!(link.as_deref(), Some("next.md"));
    assert_eq!(before.styles[0].1.font_weight, Some(FontWeight::BOLD));
    assert_eq!(after.styles[0].1.font_weight, Some(FontWeight::BOLD));
}

#[test]
fn gfm_web_autolinks_skip_code_and_exclude_surrounding_punctuation() {
    let md = Markdown::parse(
        "See (https://example.test/path(a)). Also www.example.test, not `https://code.test`.",
    );
    let [Node::Group(Tag::Paragraph, children)] = md.0.as_slice() else {
        panic!("paragraph expected")
    };
    let parts = inline::parts(children, Palette::new("#ec4899", "#111016"), true);
    let [inline::Part::Text(text)] = parts.as_slice() else {
        panic!("text expected")
    };
    let links: Vec<_> = text
        .links
        .iter()
        .map(|(range, href)| (&text.text[range.clone()], href.as_str()))
        .collect();
    assert_eq!(
        links,
        [
            (
                "https://example.test/path(a)",
                "https://example.test/path(a)"
            ),
            ("www.example.test", "http://www.example.test")
        ]
    );
}

#[test]
fn removes_only_a_complete_frontmatter_block() {
    assert_eq!(
        without_frontmatter("\u{feff}---\r\nvalue: x\r\n---\r\n# Note"),
        "# Note"
    );
    assert_eq!(without_frontmatter("---\rtitle: x\r...\r# Note"), "# Note");
    assert_eq!(
        without_frontmatter("---\nNo closing mark"),
        "---\nNo closing mark"
    );
}
