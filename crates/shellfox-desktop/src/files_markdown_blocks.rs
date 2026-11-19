use super::*;
use pulldown_cmark::HeadingLevel;

impl Renderer<'_, '_> {
    pub(super) fn block(&mut self, node: &Node) -> AnyElement {
        let palette = self.palette;
        match node {
            Node::Group(Tag::Paragraph, children) => div()
                .mb(px(10.))
                .child(self.inline(children))
                .into_any_element(),
            Node::Group(Tag::Heading { level, .. }, children) => {
                let font = match level {
                    HeadingLevel::H1 => 20.,
                    HeadingLevel::H2 => 17.,
                    HeadingLevel::H3 => 14.,
                    _ => 12.,
                };
                div()
                    .mt(px(14.))
                    .mb(px(8.))
                    .text_size(px(font))
                    .line_height(px(font * 1.25))
                    .font_weight(FontWeight::BOLD)
                    .child(self.inline(children))
                    .into_any_element()
            }
            Node::Group(Tag::CodeBlock(_), children) => div()
                .id(("md-code", self.id()))
                .overflow_x_scroll()
                .font_family("Cascadia Code")
                .p(px(8.))
                .bg(palette.sidebar)
                .border_1()
                .border_color(palette.border)
                .child(self.inline(children))
                .into_any_element(),
            Node::Group(Tag::BlockQuote(_), children) => div()
                .pl(px(10.))
                .pr(px(10.))
                .border_l(px(2.))
                .border_color(palette.border)
                .text_color(palette.muted)
                .children(children.iter().map(|child| self.block(child)))
                .into_any_element(),
            Node::Group(Tag::List(number), children) => div()
                .pl(px(40.))
                .mt(px(12.))
                .mb(px(12.))
                .flex()
                .flex_col()
                .children(children.iter().enumerate().map(|(index, item)| {
                    let label = match number {
                        Some(first) => format!("{}.", first + index as u64),
                        None => "•".into(),
                    };
                    div()
                        .flex()
                        .gap(px(5.))
                        .child(div().flex_none().child(label))
                        .child(div().min_w_0().flex_1().child(self.block(item)))
                }))
                .into_any_element(),
            Node::Group(Tag::Table(align), children) => self.table(align, children),
            Node::Group(Tag::Item | Tag::HtmlBlock, children) => {
                let mut elements = Vec::new();
                let mut start = 0;
                for (i, child) in children.iter().enumerate() {
                    if matches!(
                        child,
                        Node::Group(
                            Tag::Paragraph | Tag::List(_) | Tag::BlockQuote(_) | Tag::CodeBlock(_),
                            _
                        )
                    ) {
                        if start < i {
                            elements.push(self.inline(&children[start..i]));
                        }
                        elements.push(self.block(child));
                        start = i + 1;
                    }
                }
                if start < children.len() {
                    elements.push(self.inline(&children[start..]));
                }
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .children(elements)
                    .into_any_element()
            }
            Node::Atom(Event::Rule) => div()
                .border_t_1()
                .border_color(palette.border)
                .my(px(6.))
                .into_any_element(),
            _ => self.inline(std::slice::from_ref(node)),
        }
    }

    fn table(&mut self, align: &[pulldown_cmark::Alignment], rows: &[Node]) -> AnyElement {
        let mut widths = vec![13.0_f32; align.len()];
        let palette = self.palette;
        let mut font = gpui::font(crate::ui_font::family());
        for row in rows {
            let Node::Group(kind, cells) = row else {
                continue;
            };
            font.weight = if matches!(kind, Tag::TableHead) {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            };
            for (index, cell) in cells.iter().enumerate().take(widths.len()) {
                let Node::Group(_, children) = cell else {
                    continue;
                };
                let mut plain = Inline::default();
                for child in children {
                    plain.push(child, HighlightStyle::default(), palette);
                }
                let width = self
                    .window
                    .text_system()
                    .shape_line(
                        plain.text.clone().into(),
                        px(12.),
                        &[gpui::TextRun {
                            len: plain.text.len(),
                            font: font.clone(),
                            color: palette.foreground,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                    .width;
                widths[index] = widths[index].max(f32::from(width).ceil() + 13.);
            }
        }
        div()
            .id(("md-table", self.id()))
            .overflow_x_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(widths.iter().sum::<f32>() + 1.))
                    .border_b_1()
                    .border_r_1()
                    .border_color(palette.border)
                    .children(rows.iter().filter_map(|row| {
                        let Node::Group(kind, cells) = row else {
                            return None;
                        };
                        let heading = matches!(kind, Tag::TableHead);
                        Some(
                            div()
                                .flex()
                                .when(heading, |d| d.font_weight(FontWeight::BOLD))
                                .children(cells.iter().zip(widths.iter().zip(align)).filter_map(
                                    |(cell, (&width, alignment))| {
                                        let Node::Group(_, children) = cell else {
                                            return None;
                                        };
                                        let alignment = match alignment {
                                            pulldown_cmark::Alignment::Right => {
                                                gpui::TextAlign::Right
                                            }
                                            pulldown_cmark::Alignment::Center => {
                                                gpui::TextAlign::Center
                                            }
                                            pulldown_cmark::Alignment::None if heading => {
                                                gpui::TextAlign::Center
                                            }
                                            _ => gpui::TextAlign::Left,
                                        };
                                        Some(
                                            div()
                                                .flex_shrink_0()
                                                .w(px(width))
                                                .px(px(6.))
                                                .py(px(3.))
                                                .text_align(alignment)
                                                .border_l_1()
                                                .border_t_1()
                                                .border_color(palette.border)
                                                .child(self.inline(children)),
                                        )
                                    },
                                )),
                        )
                    })),
            )
            .into_any_element()
    }
}
