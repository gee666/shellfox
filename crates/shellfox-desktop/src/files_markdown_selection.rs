use gpui::{
    AnyElement, Div, FocusHandle, HighlightStyle, Hsla, MouseButton, Stateful, TextLayout, div,
    prelude::*,
};
use std::{cell::RefCell, ops::Range, rc::Rc};

#[derive(Clone, Default)]
pub(super) struct Selection(Rc<RefCell<SelectionState>>);

#[derive(Default)]
struct SelectionState {
    text: String,
    focus: Option<FocusHandle>,
    anchor: usize,
    head: usize,
    dragging: bool,
}

impl Selection {
    pub fn start_frame(&self, cx: &mut gpui::App) -> FocusHandle {
        let mut state = self.0.borrow_mut();
        state.text.clear();
        state.focus.get_or_insert_with(|| cx.focus_handle()).clone()
    }

    pub fn append(&self, text: &str, new_block: bool) -> Range<usize> {
        let mut state = self.0.borrow_mut();
        if new_block && !state.text.is_empty() {
            state.text.push('\n');
        }
        let start = state.text.len();
        state.text.push_str(text);
        start..state.text.len()
    }

    pub fn select_all(&self) {
        let mut state = self.0.borrow_mut();
        state.anchor = 0;
        state.head = state.text.len();
    }

    pub fn has_text(&self) -> bool {
        let state = self.0.borrow();
        state.anchor != state.head
    }

    pub fn copy(&self, cx: &mut gpui::App) -> bool {
        let state = self.0.borrow();
        let Some(text) = state.text.get(range(state.anchor, state.head)) else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_owned()));
        true
    }

    pub fn highlights(
        &self,
        block: &Range<usize>,
        styles: Vec<(Range<usize>, HighlightStyle)>,
        background: Hsla,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let state = self.0.borrow();
        let selection = range(state.anchor, state.head);
        let start = selection.start.max(block.start);
        let end = selection.end.min(block.end);
        if start >= end || !state.text.is_char_boundary(start) || !state.text.is_char_boundary(end)
        {
            return styles;
        }
        select_styles(
            styles,
            (start - block.start)..(end - block.start),
            block.len(),
            background,
        )
    }

    pub fn interactive(
        &self,
        block: Range<usize>,
        layout: TextLayout,
        child: AnyElement,
    ) -> Stateful<Div> {
        let click = self.clone();
        let drag = self.clone();
        let finish = self.clone();
        let end_outside = self.clone();
        let click_layout = layout.clone();
        let start = block.start;
        let size = block.len();
        div()
            .id(("markdown-select", block.start))
            .min_w_0()
            .child(child)
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let index = click_layout
                    .index_for_position(event.position)
                    .unwrap_or_else(|index| index)
                    .min(size)
                    + start;
                let mut state = click.0.borrow_mut();
                if let Some(focus) = &state.focus {
                    focus.focus(window);
                }
                if !event.modifiers.shift {
                    state.anchor = index;
                }
                state.head = index;
                state.dragging = true;
                cx.refresh_windows();
            })
            .on_mouse_move(move |event, _, cx| {
                let mut state = drag.0.borrow_mut();
                if event.pressed_button != Some(MouseButton::Left) {
                    state.dragging = false;
                }
                if state.dragging {
                    let index = layout
                        .index_for_position(event.position)
                        .unwrap_or_else(|index| index)
                        .min(size)
                        + start;
                    if state.head != index {
                        state.head = index;
                        cx.refresh_windows();
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                finish.0.borrow_mut().dragging = false
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, _| {
                end_outside.0.borrow_mut().dragging = false
            })
    }
}

fn range(a: usize, b: usize) -> Range<usize> {
    a.min(b)..a.max(b)
}

fn select_styles(
    styles: Vec<(Range<usize>, HighlightStyle)>,
    selected: Range<usize>,
    len: usize,
    background: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut output = Vec::new();
    let mut add = |span: Range<usize>, style: HighlightStyle| {
        if span.is_empty() {
            return;
        }
        let start = span.start.max(selected.start);
        let end = span.end.min(selected.end);
        if start >= end {
            output.push((span, style));
            return;
        }
        if span.start < start {
            output.push((span.start..start, style));
        }
        output.push((
            start..end,
            HighlightStyle {
                background_color: Some(background),
                ..style
            },
        ));
        if end < span.end {
            output.push((end..span.end, style));
        }
    };
    let mut current = 0;
    for (span, style) in styles {
        add(current..span.start, HighlightStyle::default());
        current = span.end;
        add(span, style);
    }
    add(current..len, HighlightStyle::default());
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copying_inline_image_neighbors_does_not_insert_a_block_break() {
        let selection = Selection::default();
        assert_eq!(selection.append("Before ", true), 0..7);
        assert_eq!(selection.append(" after", false), 7..13);
        assert_eq!(selection.append("Next block", true), 14..24);
        selection.select_all();
        let state = selection.0.borrow();
        assert_eq!(
            &state.text[range(state.anchor, state.head)],
            "Before  after\nNext block"
        );
    }

    #[test]
    fn overlays_selection_without_losing_inline_style() {
        let accent = gpui::rgb(0xff0000).into();
        let strong = HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..Default::default()
        };
        let styles = select_styles(vec![(2..5, strong)], 1..4, 7, accent);
        assert_eq!(
            styles
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            [0..1, 1..2, 2..4, 4..5, 5..7]
        );
        assert_eq!(styles[2].1.font_weight, strong.font_weight);
        assert_eq!(styles[2].1.background_color, Some(accent));
        assert_eq!(styles[3].1.background_color, None);
    }
}
