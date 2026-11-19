use gpui::{Pixels, SharedString, TextStyle, Window, px};

pub(super) fn tab_label_text(
    window: &Window,
    title: SharedString,
    administrator: bool,
    closeable: bool,
) -> (SharedString, Option<Pixels>) {
    // A tab caps at 180 px. The label reserves padding, dot and gap (32), the
    // right separator (1), and optional close control or badge.
    let width = px(147. - if closeable { 26. } else { 0. } - if administrator { 41. } else { 0. });
    let style = TextStyle {
        font_family: crate::ui_font::family().into(),
        ..Default::default()
    };
    let mut runs = vec![style.to_run(title.len())];
    // Flex layout initially measures the text without its cap. Shorten the
    // visible label while keeping the original title for tooltips and UIA.
    let visible = window
        .text_system()
        .line_wrapper(style.font(), px(12.))
        .truncate_line(title.clone(), width, "…", &mut runs);
    let truncated = visible != title;
    (visible, truncated.then_some(width))
}
