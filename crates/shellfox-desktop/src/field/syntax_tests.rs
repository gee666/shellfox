use super::*;
#[test]
fn every_prism_extension_has_a_valid_grammar() {
    for ext in [
        "js", "mjs", "cjs", "jsx", "ts", "tsx", "html", "htm", "xml", "svg", "css", "json",
        "sh", "bash", "zsh", "py", "ps1", "psm1", "cs", "md", "markdown", "mdx", "yml", "yaml",
    ] {
        assert!(
            Language::for_path(&format!("fixture.{ext}"))
                .unwrap()
                .config()
                .is_some(),
            "{ext}"
        );
    }
    assert!(Language::for_path("fixture.rs").is_none());
}
#[test]
fn selects_prism_colors_with_source_byte_boundaries_and_size_limit() {
    let source = "// 雪\nconst x = 8; console.log(\"hi\");";
    let spans = highlight(Language::Js, source);
    for (text, token) in [
        ("// 雪", Token::Comment),
        ("const", Token::Keyword),
        ("8", Token::Number),
        ("log", Token::Number),
        ("\"hi\"", Token::String),
    ] {
        assert!(spans.iter().any(|span| source.get(span.range.clone()) == Some(text) && span.token == token), "{text}: {spans:?}");
    }
    assert!(highlight(Language::Js, &"雪".repeat(10_001)).is_empty());
    assert!(highlight(Language::Js, &"x\n".repeat(100_001)).is_empty());
}
