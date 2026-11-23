use super::markdown_image::MarkdownImage;
use crate::{files::DocumentBackend, root::Root, theme::Palette};
use gpui::{AnyElement, ClickEvent, Context, Entity, FontWeight, StyledText, div, prelude::*, px};
use pulldown_cmark::{Event, Options, Parser, Tag};
mod anchors;
mod blocks;
mod inline;
mod selection;
use std::collections::HashMap;
pub(super) type Images = HashMap<(String, String), Entity<MarkdownImage>>;

#[derive(Default)]
pub(super) struct Markdown(Vec<Node>, anchors::Anchors, selection::Selection);

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
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES;
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
        Self(root, Default::default(), Default::default())
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
        self.1.clear();
        let focus = self.2.start_frame(cx);
        let selection = self.2.clone();
        let mut renderer = Renderer {
            anchors: &self.1,
            selection: &self.2,
            document_path,
            backend,
            images,
            palette,
            window,
            cx,
            next_id: 0,
            block_index: 0,
        };
        div()
            .id("file-markdown-preview")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.1.scroll)
            .track_focus(&focus)
            .on_key_down(move |event, window, cx| {
                let modifiers = event.keystroke.modifiers;
                let primary = if cfg!(target_os = "macos") {
                    modifiers.platform
                } else {
                    modifiers.control
                };
                if !primary || modifiers.alt || modifiers.shift {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "a" => {
                        selection.select_all();
                        cx.refresh_windows();
                    }
                    "c" if selection.copy(cx) => {}
                    _ => return,
                }
                window.prevent_default();
                cx.stop_propagation();
            })
            .flex()
            .flex_col()
            .min_w_0()
            .px(px(16.))
            .py(px(12.))
            .text_size(px(12.))
            .line_height(px(18.6))
            .children(self.0.iter().enumerate().map(|(index, node)| {
                renderer.block_index = index;
                renderer.block(node)
            }))
    }
}

struct Renderer<'a, 'b> {
    anchors: &'a anchors::Anchors,
    selection: &'a selection::Selection,
    document_path: &'a str,
    backend: &'a DocumentBackend,
    images: &'a mut Images,
    palette: Palette,
    window: &'a gpui::Window,
    cx: &'a mut Context<'b, Root>,
    next_id: usize,
    block_index: usize,
}

impl Renderer<'_, '_> {
    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn inline(&mut self, nodes: &[Node]) -> AnyElement {
        self.inline_content(nodes, true)
    }

    fn inline_content(&mut self, nodes: &[Node], autolink: bool) -> AnyElement {
        let mut parts = inline::parts(nodes, self.palette, autolink);
        if parts.len() == 1
            && matches!(parts[0], inline::Part::Text(_))
            && let Some(inline::Part::Text(text)) = parts.pop()
        {
            return self.styled_inline(text, true);
        }
        let mut new_block = true;
        div()
            .flex()
            .flex_wrap()
            .min_w_0()
            .items_baseline()
            .children(parts.into_iter().map(|part| match part {
                inline::Part::Text(text) => {
                    self.styled_inline(text, std::mem::take(&mut new_block))
                }
                inline::Part::Image { source, alt, link } => self.inline_image(source, alt, link),
            }))
            .into_any_element()
    }

    fn inline_image(&mut self, source: String, alt: String, link: Option<String>) -> AnyElement {
        let key = (source, alt);
        let asset = self
            .images
            .entry(key.clone())
            .or_insert_with(|| {
                MarkdownImage::load(
                    self.backend.clone(),
                    self.document_path.to_owned(),
                    key.0,
                    key.1,
                    self.cx,
                )
            })
            .clone();
        let image = div().min_w_0().max_w_full().child(asset);
        if let Some(link) = link {
            let activate = self.link_handler();
            return image
                .id(("md-image-link", self.id()))
                .cursor_pointer()
                .on_click(move |_, _, cx| activate(&link, cx))
                .into_any_element();
        }
        image.into_any_element()
    }

    fn link_handler(&self) -> impl Fn(&str, &mut gpui::App) + use<> {
        let entity = self.cx.entity();
        let path = self.document_path.to_owned();
        let anchors = self.anchors.clone();
        move |href, cx| match link_destination(&path, href) {
            Some(Link::External(url)) => cx.open_url(&url),
            Some(Link::Anchor(id)) => {
                if anchors.jump(&id) {
                    cx.refresh_windows();
                }
            }
            Some(Link::File(path)) => {
                entity.update(cx, |root, cx| root.request_open_file(path, cx))
            }
            None => {}
        }
    }

    fn styled_inline(&mut self, inline: inline::Inline, new_block: bool) -> AnyElement {
        let block = self.selection.append(&inline.text, new_block);
        let background = gpui::Hsla {
            a: 0.35,
            ..self.palette.accent
        };
        let styles = self.selection.highlights(&block, inline.styles, background);
        let styled = StyledText::new(inline.text).with_highlights(styles);
        let layout = styled.layout().clone();
        let row = self
            .selection
            .interactive(block, layout.clone(), styled.into_any_element());
        if inline.links.is_empty() {
            return row.into_any_element();
        }
        let activate = self.link_handler();
        let selection = self.selection.clone();
        row.cursor_pointer()
            .on_click(move |event, _, cx| {
                let ClickEvent::Mouse(event) = event else {
                    return;
                };
                if event.up.modifiers.shift || selection.has_text() {
                    return;
                }
                let (Ok(start), Ok(end)) = (
                    layout.index_for_position(event.down.position),
                    layout.index_for_position(event.up.position),
                ) else {
                    return;
                };
                if let Some((_, href)) = inline
                    .links
                    .iter()
                    .find(|(range, _)| range.contains(&start) && range.contains(&end))
                {
                    activate(href, cx);
                }
            })
            .into_any_element()
    }
}

fn without_frontmatter(text: &str) -> &str {
    let trimmed = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = trimmed.split_inclusive(['\r', '\n']);
    let Some(first) = lines.next() else {
        return text;
    };
    if first.trim_end_matches(['\r', '\n', '\t', ' ']) != "---" {
        return text;
    }
    let mut consumed = first.len();
    for line in lines {
        consumed += line.len();
        if matches!(
            line.trim_end_matches(['\r', '\n', '\t', ' ']),
            "---" | "..."
        ) {
            let rest = &trimmed[consumed..];
            return if line.ends_with('\r') {
                rest.strip_prefix('\n').unwrap_or(rest)
            } else {
                rest
            };
        }
    }
    text
}

#[derive(Debug, PartialEq, Eq)]
enum Link {
    External(String),
    File(String),
    Anchor(String),
}
fn link_destination(path: &str, href: &str) -> Option<Link> {
    if href.is_empty() {
        return None;
    }
    if let Some(anchor) = href.strip_prefix('#') {
        return Some(Link::Anchor(
            percent_encoding::percent_decode_str(anchor)
                .decode_utf8()
                .ok()?
                .into_owned(),
        ));
    }
    if let Ok(url) = url::Url::parse(href) {
        let has_web_prefix = href.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        });
        return (has_web_prefix
            && url.has_host()
            && href.encode_utf16().take(2049).count() <= 2048
            && !href.chars().any(char::is_control))
        .then(|| Link::External(url.into()));
    }
    let path_end = path.rfind(['/', '\\']).map_or(0, |index| index + 1);
    let target = percent_encoding::percent_decode_str(href.split('#').next()?)
        .decode_utf8()
        .ok()?;
    if target.starts_with(['/', '\\'])
        || target.as_bytes().get(1) == Some(&b':')
        || target.chars().any(char::is_control)
    {
        return None;
    }
    Some(Link::File(format!("{}{target}", &path[..path_end])))
}

#[cfg(test)]
mod tests;
