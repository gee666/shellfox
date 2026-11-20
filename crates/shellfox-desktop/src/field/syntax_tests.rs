use super::*;
#[test]
fn every_prism_extension_has_a_valid_grammar() {
    for ext in [
        "js", "mjs", "cjs", "jsx", "ts", "tsx", "html", "htm", "xml", "svg", "css", "json", "sh",
        "bash", "zsh", "py", "ps1", "psm1", "cs", "md", "markdown", "mdx", "yml", "yaml",
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
        assert!(
            spans
                .iter()
                .any(|span| source.get(span.range.clone()) == Some(text) && span.token == token),
            "{text}: {spans:?}"
        );
    }
    assert!(highlight(Language::Js, &"雪".repeat(10_001)).is_empty());
    assert!(highlight(Language::Js, &"x\n".repeat(100_001)).is_empty());
}

#[test]
fn powershell_booleans_get_the_keyword_color_but_similar_variables_do_not() {
    let source = "$ready = $TrUe; $other = $false; $name = $trueBlue";
    let spans = highlight(Language::PowerShell, source);
    let keywords: Vec<_> = spans
        .iter()
        .filter(|span| span.token == Token::Keyword)
        .map(|span| &source[span.range.clone()])
        .collect();
    assert_eq!(keywords, ["$TrUe", "$false"]);
}

#[test]
fn cutoff_counts_utf16_units_not_unicode_scalars_or_utf8_bytes() {
    let exact = format!("//{}", "🦊".repeat(4999));
    assert!(!highlight(Language::Js, &exact).is_empty());
    assert!(highlight(Language::Js, &(exact + "x")).is_empty());
}

#[test]
fn common_token_colors_match_the_editor_reference() {
    use Language::*;
    for (language, source, sample, expected) in [
        (
            Json,
            r#"{"ready": true}"#,
            r#""ready""#,
            Some(Token::Property),
        ),
        (Json, r#"{"ready": true}"#, "true", Some(Token::Keyword)),
        (Html, "<div>", "<", Some(Token::Keyword)),
        (Jsx, "<span/>", "/>", Some(Token::Keyword)),
        (Xml, "<svg/>", "/>", Some(Token::Keyword)),
        (Css, ".item { width: 4px; }", "4", None),
        (Bash, "printf '%s' hi", "printf", None),
        (Python, "print(True)", "print", Some(Token::Keyword)),
        (Python, "print(True)", "True", Some(Token::Keyword)),
        (
            CSharp,
            "class App { string Message; }",
            "string",
            Some(Token::Keyword),
        ),
        (Yaml, "ready: true", "ready", None),
    ] {
        let start = source.find(sample).unwrap();
        let range = start..start + sample.len();
        let spans = highlight(language, source);
        let color = spans
            .iter()
            .find(|span| span.range.start <= range.start && span.range.end >= range.end)
            .map(|span| span.token);
        assert_eq!(color, expected, "{sample} in {source}: {spans:?}");
    }
}

#[test]
fn common_language_samples_have_visible_highlights() {
    for (path, source) in [
        ("source.tsx", "const node = <span title='hi'>hello</span>;"),
        ("source.html", "<div class='greet'>hi</div>"),
        ("source.svg", "<svg width='48'/>"),
        ("source.css", ".note { color: #ff0000; padding: 4px; }"),
        ("source.json", "{\"ready\": true, \"count\": 42}"),
        ("source.sh", "# note\nprintf '%s' hello"),
        ("source.py", "# note\nprint('hello')"),
        ("source.cs", "// note\npublic class App {}"),
        ("source.md", "# Note\n\n<!-- private -->"),
        ("source.yml", "key: true\nport: 42"),
    ] {
        assert!(
            !highlight(Language::for_path(path).unwrap(), source).is_empty(),
            "{path}"
        );
    }
}
