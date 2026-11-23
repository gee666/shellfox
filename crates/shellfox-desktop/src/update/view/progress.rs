use crate::theme::Palette;
use gpui::{Animation, AnimationExt, AnyElement, div, prelude::*, px, relative};
use std::time::Duration;

/// A thin accent bar on the input-border track. Unknown totals sweep back and forth.
pub(super) fn progress(received: u64, total: u64, palette: Palette) -> AnyElement {
    let segment = div().absolute().top_0().bottom_0().bg(palette.accent);
    let value = if total == 0 {
        segment
            .w(relative(0.20))
            .with_animation(
                "update-indeterminate",
                Animation::new(Duration::from_millis(2200)).repeat(),
                |segment, time| segment.left(relative((1. - (time * 2. - 1.).abs()) * 0.8)),
            )
            .into_any_element()
    } else {
        segment
            .left_0()
            .w(relative((received.min(total) as f64 / total as f64) as f32))
            .when(received > 0, |segment| segment.min_w(px(2.)))
            .into_any_element()
    };
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(3.))
        .rounded_full()
        .bg(palette.input_border)
        .overflow_hidden()
        .child(value)
        .into_any_element()
}
