use super::markdown_image::MarkdownImage;
use crate::{files::DocumentBackend, root::Root, theme::Palette};
use gpui::{
    AnyElement, Context, Entity, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    StyledText, div, prelude::*, px,
};
use pulldown_cmark::{Event, Options, Parser, Tag};
#[path = "files_markdown_blocks.rs"]
mod blocks;
use std::{collections::HashMap, ops::Range};
pub(super) type Images = HashMap<(String, String), Entity<MarkdownImage>>;

#[derive(Default)]
pub(super) struct Markdown(Vec<Node>);

enum Node {
    Group(Tag<'static>, Vec<Node>),
    Atom(Event<'static>),
}

impl Markdown {
    pub fn parse(source: &str) -> Self {
        let mut root = Vec::new();
        let mut stack = Vec::new();
        let options = Options::ENABLE_GFM
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TABLES
            | Options::ENABLE_TASKLISTS;
        for event in Parser::new_ext(without_frontmatter(source), options) {
            let node = match event.into_static() {
                Event::Start(tag) => {
                    stack.push((tag, Vec::new()));
                    continue;
                }
                Event::End(_) => {
                    let Some((tag, children)) = stack.pop() else {
                        continue;
                    };
                    Node::Group(tag, children)
                }
                event => Node::Atom(event),
            };
            stack
                .last_mut()
                .map_or(&mut root, |(_, children)| children)
                .push(node);
        }
        Self(root)
    }

    pub fn render(
        &self,
        document_path: &str,
        backend: &DocumentBackend,
        images: &mut Images,
        palette: Palette,
        window: &gpui::Window,
        cx: &mut Context<Root>,
    ) -> impl IntoElement {
        let mut renderer = Renderer {
            document_path,
            backend,
            images,
            palette,
            window,
            cx,
            next_id: 0,
        };
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .px(px(16.))
            .py(px(12.))
            .text_size(px(12.))
            .line_height(px(18.6))
            .children(self.0.iter().map(|node| renderer.block(node)))
    }
}

struct Renderer<'a, 'b> {
    document_path: &'a str,
    backend: &'a DocumentBackend,
    images: &'a mut Images,
    palette: Palette,
    window: &'a gpui::Window,
    cx: &'a mut Context<'b, Root>,
    next_id: usize,
}

impl Renderer<'_, '_> {
    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn inline(&mut self, nodes: &[Node]) -> AnyElement {
        if !nodes
            .iter()
            .any(|node| matches!(node, Node::Group(Tag::Image { .. }, _)))
        {
            return self.styled_inline(nodes);
        }
        let mut parts = Vec::new();
        let mut start = 0;
        for (index, node) in nodes.iter().enumerate() {
            let Node::Group(Tag::Image { dest_url, .. }, children) = node else {
                continue;
            };
            if start < index {
                parts.push(self.styled_inline(&nodes[start..index]));
            }
            let mut alt = Inline::default();
            for child in children {
                alt.push(child, HighlightStyle::default(), self.palette);
            }
            let name = if alt.text.is_empty() {
                dest_url.to_string()
            } else {
                alt.text
            };
            let key = (dest_url.to_string(), name);
            let asset = self.images.entry(key.clone()).or_insert_with(|| {
                MarkdownImage::load(
                    self.backend.clone(),
                    self.document_path.to_owned(),
                    key.0,
                    key.1,
                    self.cx,
                )
            });
            parts.push(asset.clone().into_any_element());
            start = index + 1;
        }
        if start < nodes.len() {
            parts.push(self.styled_inline(&nodes[start..]));
        }
        div()
            .flex()
            .flex_wrap()
            .min_w_0()
            .items_center()
            .children(parts)
            .into_any_element()
    }

    fn styled_inline(&mut self, nodes: &[Node]) -> AnyElement {
        let mut inline = Inline::default();
        for node in nodes {
            inline.push(node, HighlightStyle::default(), self.palette);
        }
        let styled = StyledText::new(inline.text).with_highlights(inline.styles);
        if inline.links.is_empty() {
            return styled.into_any_element();
        }
        let (ranges, links): (Vec<_>, Vec<_>) = inline.links.into_iter().unzip();
        let id = self.id();
        let entity = self.cx.entity();
        let path = self.document_path.to_owned();
        InteractiveText::new(("md-link", id), styled)
            .on_click(ranges, move |index, _, cx| {
                let Some(href) = links.get(index) else {
                    return;
                };
                match link_destination(&path, href) {
                    Some(Link::External(url)) => cx.open_url(&url),
                    Some(Link::File(path)) => {
                        entity.update(cx, |root, cx| root.request_open_file(path, cx))
                    }
                    None => {}
                }
            })
            .into_any_element()
    }
}

#[derive(Default)]
struct Inline {
    text: String,
    styles: Vec<(Range<usize>, HighlightStyle)>,
    links: Vec<(Range<usize>, String)>,
}
impl Inline {
    fn push(&mut self, node: &Node, mut style: HighlightStyle, palette: Palette) {
        match node {
            Node::Group(tag, children) => {
                match tag {
                    Tag::Strong => style.font_weight = Some(FontWeight::BOLD),
                    Tag::Emphasis => style.font_style = Some(FontStyle::Italic),
                    Tag::Strikethrough => {
                        style.strikethrough = Some(gpui::StrikethroughStyle {
                            thickness: px(1.),
                            color: None,
                        })
                    }
                    Tag::Link { .. } => {
                        style.color = Some(palette.accent);
                        style.underline = Some(gpui::UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: false,
                        });
                    }
                    _ => {}
                }
                let start = self.text.len();
                if let Tag::Image { dest_url, .. } = tag {
                    self.text.push_str("Image unavailable: ");
                    if children.is_empty() {
                        self.text.push_str(dest_url);
                    }
                }
                for child in children {
                    self.push(child, style, palette);
                }
                if let Tag::Link { dest_url, .. } = tag
                    && self.text.len() > start
                {
                    self.links
                        .push((start..self.text.len(), dest_url.to_string()));
                }
            }
            Node::Atom(event) => {
                let start = self.text.len();
                match event {
                    Event::Text(text)
                    | Event::Code(text)
                    | Event::Html(text)
                    | Event::InlineHtml(text)
                    | Event::InlineMath(text)
                    | Event::DisplayMath(text) => self.text.push_str(text),
                    Event::TaskListMarker(checked) => {
                        self.text.push_str(if *checked { "☑ " } else { "☐ " })
                    }
                    Event::SoftBreak => self.text.push(' '),
                    Event::HardBreak => self.text.push('\n'),
                    _ => {}
                }
                if self.text.len() > start && style != HighlightStyle::default() {
                    self.styles.push((start..self.text.len(), style));
                }
            }
        }
    }
}

fn without_frontmatter(text: &str) -> &str {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = trimmed.split_inclusive('\n');
    if !lines
        .next()
        .is_some_and(|line| line.trim_end_matches(['\r', '\n', '\t', ' ']) == "---")
    {
        return text;
    }
    let mut consumed = trimmed.find('\n').map_or(trimmed.len(), |end| end + 1);
    for line in lines {
        consumed += line.len();
        if matches!(
            line.trim_end_matches(['\r', '\n', '\t', ' ']),
            "---" | "..."
        ) {
            return &trimmed[consumed..];
        }
    }
    text
}

#[derive(Debug, PartialEq, Eq)]
enum Link {
    External(String),
    File(String),
}
fn link_destination(path: &str, href: &str) -> Option<Link> {
    if href.is_empty() || href.starts_with('#') {
        return None;
    }
    if let Ok(url) = url::Url::parse(href) {
        return matches!(url.scheme(), "http" | "https").then(|| Link::External(url.into()));
    }
    let path_end = path.rfind(['/', '\\']).map_or(0, |index| index + 1);
    let target = percent_encoding::percent_decode_str(href.split('#').next()?)
        .decode_utf8()
        .ok()?;
    Some(Link::File(format!("{}{target}", &path[..path_end])))
}

#[cfg(test)]
#[path = "files_markdown_tests.rs"]
mod tests;
