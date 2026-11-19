use gpui::{Font, ShapedLine, Window, font, px};

pub(super) fn base_font(monospace: bool, font_size: f32, code: bool, window: &Window) -> Font {
    let mut selected = if monospace {
        crate::renderer::choose_font(window, "monospace", font_size)
            .map(|(font, _, _)| font)
            .unwrap_or_else(|_| font(".SystemUIFont"))
    } else {
        font(crate::ui_font::family())
    };
    if code {
        selected.features = gpui::FontFeatures(std::sync::Arc::new(
            ["liga", "clig", "dlig", "hlig", "calt"]
                .into_iter()
                .map(|name| (name.to_owned(), 0))
                .collect(),
        ));
    }
    selected
}
pub(super) fn shape(
    text: &str,
    font: &Font,
    color: gpui::Hsla,
    font_size: f32,
    window: &Window,
) -> ShapedLine {
    window.text_system().shape_line(
        text.to_owned().into(),
        px(font_size),
        &[gpui::TextRun {
            len: text.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}
