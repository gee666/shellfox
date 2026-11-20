use super::{Node, Palette};
use gpui::{FontStyle, FontWeight, HighlightStyle, StrikethroughStyle, UnderlineStyle, px};
use pulldown_cmark::{Event, Tag};
use std::ops::Range;

#[derive(Default)]
pub(super) struct Inline {
    pub text: String,
    pub styles: Vec<(Range<usize>, HighlightStyle)>,
    pub links: Vec<(Range<usize>, String)>,
}

pub(super) enum Part {
    Text(Inline),
    Image {
        source: String,
        alt: String,
        link: Option<String>,
    },
}

#[derive(Default)]
struct Flow {
    text: Inline,
    parts: Vec<Part>,
}

pub(super) fn parts(nodes: &[Node], palette: Palette, autolink: bool) -> Vec<Part> {
    let mut flow = Flow::default();
    flow.visit(nodes, HighlightStyle::default(), None, palette, autolink);
    flow.flush();
    flow.parts
}

pub(super) fn plain(nodes: &[Node]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            Node::Group(_, children) => text.push_str(&plain(children)),
            Node::Atom(event) => text.push_str(display(event)),
        }
    }
    text
}

impl Flow {
    fn flush(&mut self) {
        if !self.text.text.is_empty() {
            self.parts.push(Part::Text(std::mem::take(&mut self.text)));
        }
    }

    fn visit(
        &mut self,
        nodes: &[Node],
        style: HighlightStyle,
        link: Option<&str>,
        palette: Palette,
        autolink: bool,
    ) {
        for node in nodes {
            match node {
                Node::Group(Tag::Image { dest_url, .. }, children) => {
                    self.flush();
                    let alt = plain(children);
                    self.parts.push(Part::Image {
                        source: dest_url.to_string(),
                        alt: if alt.is_empty() {
                            dest_url.to_string()
                        } else {
                            alt
                        },
                        link: link.map(str::to_owned),
                    });
                }
                Node::Group(tag, children) => {
                    let mut nested = style;
                    let mut link = link;
                    match tag {
                        Tag::Strong => nested.font_weight = Some(FontWeight::BOLD),
                        Tag::Emphasis => nested.font_style = Some(FontStyle::Italic),
                        Tag::Strikethrough => {
                            nested.strikethrough = Some(StrikethroughStyle {
                                thickness: px(1.),
                                color: None,
                            })
                        }
                        Tag::Link { dest_url, .. } => {
                            nested = link_style(nested, palette);
                            link = Some(dest_url);
                        }
                        _ => {}
                    }
                    self.visit(children, nested, link, palette, autolink);
                }
                Node::Atom(event) => {
                    let value = display(event);
                    if autolink && matches!(event, Event::Text(_)) && link.is_none() {
                        self.text.autolink(value, style, palette);
                    } else {
                        self.text.push(value, style, link);
                    }
                }
            }
        }
    }
}

impl Inline {
    fn push(&mut self, text: &str, style: HighlightStyle, href: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if style != HighlightStyle::default() {
            self.styles.push((start..end, style));
        }
        if let Some(href) = href {
            if let Some((range, previous)) = self.links.last_mut() {
                if previous == href && range.end == start {
                    range.end = end;
                    return;
                }
            }
            self.links.push((start..end, href.to_owned()));
        }
    }

    fn autolink(&mut self, text: &str, style: HighlightStyle, palette: Palette) {
        use std::sync::LazyLock;
        static URLS: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r#"(?:https?://|www\.)[^\s<>]+"#).expect("valid autolink pattern")
        });
        let mut start = 0;
        for found in URLS.find_iter(text) {
            let before = text[..found.start()].chars().next_back();
            if before.is_some_and(|ch| {
                !ch.is_whitespace() && !matches!(ch, '(' | '[' | '{' | '*' | '_' | '~')
            }) {
                continue;
            }
            let mut value = found
                .as_str()
                .trim_end_matches(['.', ',', ':', ';', '!', '?', '*', '_', '~']);
            let surplus = value
                .bytes()
                .filter(|&b| b == b')')
                .count()
                .saturating_sub(value.bytes().filter(|&b| b == b'(').count());
            for _ in 0..surplus {
                let Some(rest) = value.strip_suffix(')') else {
                    break;
                };
                value = rest;
            }
            let href = if value.starts_with("www.") {
                format!("http://{value}")
            } else {
                value.to_owned()
            };
            let Ok(url) = url::Url::parse(&href) else {
                continue;
            };
            if !url.host_str().is_some_and(|host| host.contains('.')) {
                continue;
            }
            self.push(&text[start..found.start()], style, None);
            self.push(value, link_style(style, palette), Some(&href));
            start = found.start() + value.len();
        }
        self.push(&text[start..], style, None);
    }
}

fn link_style(mut style: HighlightStyle, palette: Palette) -> HighlightStyle {
    style.color = Some(palette.accent);
    style.underline = Some(UnderlineStyle {
        thickness: px(1.),
        color: None,
        wavy: false,
    });
    style
}

fn display<'a>(event: &'a Event<'_>) -> &'a str {
    match event {
        Event::Text(text)
        | Event::Code(text)
        | Event::Html(text)
        | Event::InlineHtml(text)
        | Event::InlineMath(text)
        | Event::DisplayMath(text) => text,
        Event::TaskListMarker(true) => "☑ ",
        Event::TaskListMarker(false) => "☐ ",
        Event::SoftBreak => " ",
        Event::HardBreak => "\n",
        _ => "",
    }
}
