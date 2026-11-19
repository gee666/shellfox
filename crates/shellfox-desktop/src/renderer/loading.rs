use gpui::{div, prelude::*, px, Animation, AnimationExt, AnyElement, Hsla, IntoElement};
use std::time::Duration;

pub(crate) fn terminal_loading(accent: Hsla) -> AnyElement {
    let mut label = div()
        .flex()
        .font_family("Cascadia Code")
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(accent)
        .child("> ");
    for (id, delay) in [("dot-1", 0.), ("dot-2", 1. / 6.), ("dot-3", 1. / 3.)] {
        label = label.child(div().child(".").with_animation(
            id,
            Animation::new(Duration::from_millis(1200)).repeat(),
            move |dot, phase| {
                let phase = (phase - delay).rem_euclid(1.);
                let opacity = match (phase * 4.).floor() as u8 {
                    1 | 3 => 0.65,
                    2 => 1.,
                    _ => 0.3,
                };
                dot.opacity(opacity)
            },
        ));
    }
    div()
        .id("terminal-loading")
        .flex_1()
        .p(px(12.))
        .child(label)
        .into_any_element()
}
