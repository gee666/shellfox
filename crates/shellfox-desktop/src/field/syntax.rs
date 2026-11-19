use crate::theme::Palette;
use gpui::{Font, TextRun, rgb};
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Comment,
    String,
    Keyword,
    Number,
    Property,
}

// Keep this map in the same order as `CAPTURES`. The colors are files.css's Prism rules.
const CAPTURES: &[&str] = &[
    "comment",
    "prolog",
    "doctype",
    "string",
    "string.special.key",
    "keyword",
    "tag",
    "boolean",
    "number",
    "function",
    "property",
    "attribute",
];
const TOKENS: &[Token] = &[
    Token::Comment,
    Token::Comment,
    Token::Comment,
    Token::String,
    Token::Property,
    Token::Keyword,
    Token::Keyword,
    Token::Keyword,
    Token::Number,
    Token::Number,
    Token::Property,
    Token::Property,
];
impl Token {
    fn color(self, palette: Palette) -> gpui::Hsla {
        match self {
            Self::Comment => palette.muted,
            Self::String => rgb(0x70b980).into(),
            Self::Keyword => palette.accent,
            Self::Number => rgb(0xc99561).into(),
            Self::Property => rgb(0xb190d0).into(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Language {
    Js,
    Jsx,
    Ts,
    Tsx,
    Html,
    Xml,
    Css,
    Json,
    Bash,
    Python,
    PowerShell,
    CSharp,
    Markdown,
    MarkdownInline,
    Yaml,
}
impl Language {
    pub(super) fn for_path(path: &str) -> Option<Self> {
        use Language::*;
        let name = path.rsplit(['/', '\\']).next()?;
        Some(
            match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
                "js" | "mjs" | "cjs" => Js,
                "jsx" => Jsx,
                "ts" => Ts,
                "tsx" => Tsx,
                "html" | "htm" => Html,
                "xml" | "svg" => Xml,
                "css" => Css,
                "json" => Json,
                "sh" | "bash" | "zsh" => Bash,
                "py" => Python,
                "ps1" | "psm1" => PowerShell,
                "cs" => CSharp,
                "md" | "markdown" | "mdx" => Markdown,
                "yml" | "yaml" => Yaml,
                _ => return None,
            },
        )
    }
    fn injected(name: &str) -> Option<Self> {
        Some(match name {
            "javascript" | "js" => Self::Js,
            "typescript" | "ts" => Self::Ts,
            "markdown_inline" | "markdown-inline" => Self::MarkdownInline,
            "html" => Self::Html,
            "css" => Self::Css,
            _ => return Self::for_path(&format!(".{name}")),
        })
    }
    fn config(self) -> Option<&'static HighlightConfiguration> {
        use Language::*;
        use tree_sitter_javascript as js;
        use tree_sitter_typescript as ts;
        static LANGUAGES: [OnceLock<Option<HighlightConfiguration>>; 15] =
            [const { OnceLock::new() }; 15];
        LANGUAGES[self as usize]
            .get_or_init(|| {
                let (language, name, highlights, injections, locals) = match self {
                    Js | Jsx => (
                        js::LANGUAGE,
                        "javascript",
                        [js::HIGHLIGHT_QUERY, js::JSX_HIGHLIGHT_QUERY].join("\n"),
                        js::INJECTIONS_QUERY,
                        js::LOCALS_QUERY,
                    ),
                    Ts | Tsx => {
                        let (lang, jsx) = if matches!(self, Tsx) {
                            (ts::LANGUAGE_TSX, js::JSX_HIGHLIGHT_QUERY)
                        } else {
                            (ts::LANGUAGE_TYPESCRIPT, "")
                        };
                        (
                            lang,
                            "typescript",
                            [ts::HIGHLIGHTS_QUERY, js::HIGHLIGHT_QUERY, jsx].join("\n"),
                            "",
                            ts::LOCALS_QUERY,
                        )
                    }
                    Html => (
                        tree_sitter_html::LANGUAGE,
                        "html",
                        tree_sitter_html::HIGHLIGHTS_QUERY.into(),
                        tree_sitter_html::INJECTIONS_QUERY,
                        "",
                    ),
                    Xml => (
                        tree_sitter_xml::LANGUAGE_XML,
                        "xml",
                        tree_sitter_xml::XML_HIGHLIGHT_QUERY.into(),
                        "",
                        "",
                    ),
                    Css => (
                        tree_sitter_css::LANGUAGE,
                        "css",
                        tree_sitter_css::HIGHLIGHTS_QUERY.into(),
                        "",
                        "",
                    ),
                    Json => (
                        tree_sitter_json::LANGUAGE,
                        "json",
                        "[(true) (false)] @boolean\n".to_owned()
                            + tree_sitter_json::HIGHLIGHTS_QUERY,
                        "",
                        "",
                    ),
                    Bash => (
                        tree_sitter_bash::LANGUAGE,
                        "bash",
                        tree_sitter_bash::HIGHLIGHT_QUERY.into(),
                        "",
                        "",
                    ),
                    Python => (
                        tree_sitter_python::LANGUAGE,
                        "python",
                        "[(true) (false)] @boolean\n".to_owned()
                            + tree_sitter_python::HIGHLIGHTS_QUERY,
                        "",
                        "",
                    ),
                    PowerShell => (
                        tree_sitter_powershell::LANGUAGE,
                        "powershell",
                        tree_sitter_powershell::HIGHLIGHTS_QUERY.into(),
                        "",
                        "",
                    ),
                    CSharp => (
                        tree_sitter_c_sharp::LANGUAGE,
                        "c-sharp",
                        tree_sitter_c_sharp::HIGHLIGHTS_QUERY.into(),
                        "",
                        "",
                    ),
                    Markdown => (
                        tree_sitter_md::LANGUAGE,
                        "markdown",
                        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.into(),
                        tree_sitter_md::INJECTION_QUERY_BLOCK,
                        "",
                    ),
                    MarkdownInline => (
                        tree_sitter_md::INLINE_LANGUAGE,
                        "markdown-inline",
                        tree_sitter_md::HIGHLIGHT_QUERY_INLINE.into(),
                        "",
                        "",
                    ),
                    Yaml => (
                        tree_sitter_yaml::LANGUAGE,
                        "yaml",
                        tree_sitter_yaml::HIGHLIGHTS_QUERY.into(),
                        "",
                        "",
                    ),
                };
                match HighlightConfiguration::new(
                    language.into(),
                    name,
                    &highlights,
                    injections,
                    locals,
                ) {
                    Ok(mut config) => {
                        config.configure(CAPTURES);
                        Some(config)
                    }
                    Err(error) => {
                        eprintln!("Files highlight configuration for {name}: {error}");
                        None
                    }
                }
            })
            .as_ref()
    }
}

#[derive(Clone, Debug)]
pub(super) struct Span {
    range: Range<usize>,
    token: Token,
}

pub(super) fn highlight(language: Language, source: &str) -> Arc<[Span]> {
    if source.encode_utf16().take(200_001).count() > 200_000
        || source
            .split('\n')
            .any(|line| line.encode_utf16().take(10_001).count() > 10_000)
    {
        return Arc::new([]);
    }
    let Some(config) = language.config() else {
        return Arc::new([]);
    };
    let mut engine = Highlighter::new();
    let Ok(events) = engine.highlight(config, source.as_bytes(), None, None, |name| {
        Language::injected(name)?.config()
    }) else {
        return Arc::new([]);
    };
    let mut stack = Vec::new();
    let mut spans = Vec::new();
    for event in events {
        match event {
            Ok(HighlightEvent::HighlightStart(highlight)) => stack.push(TOKENS[highlight.0]),
            Ok(HighlightEvent::HighlightEnd) => {
                stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                if let Some(&token) = stack.last()
                    && source.is_char_boundary(start)
                    && source.is_char_boundary(end)
                {
                    spans.push(Span {
                        range: start..end,
                        token,
                    });
                }
            }
            Err(_) => return Arc::new([]),
        }
    }
    spans.into()
}

pub(super) fn line_runs(
    spans: &[Span],
    range: Range<usize>,
    font: &Font,
    palette: Palette,
    projection: Option<&super::editor_line::Projection>,
) -> Vec<TextRun> {
    let offset = |absolute| {
        let relative = absolute - range.start;
        projection.map_or(relative, |projection| projection.display_index(relative))
    };
    let run = |len, color| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut from = range.start;
    let mut runs = Vec::new();
    let first = spans.partition_point(|span| span.range.end <= from);
    for span in spans[first..]
        .iter()
        .take_while(|span| span.range.start < range.end)
    {
        let start = span.range.start.max(from);
        let end = span.range.end.min(range.end);
        if start > from {
            runs.push(run(offset(start) - offset(from), palette.foreground));
        }
        if end > start {
            runs.push(run(offset(end) - offset(start), span.token.color(palette)));
        }
        from = end;
    }
    if from < range.end {
        runs.push(run(offset(range.end) - offset(from), palette.foreground));
    }
    runs
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
