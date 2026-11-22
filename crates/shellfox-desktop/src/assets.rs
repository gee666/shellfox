use gpui::{AppContext, AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;
const ICONS: &[(&str, &[u8])] = &[
    ("more", include_bytes!("../assets/icons/more.svg")),
    (
        "update-download",
        include_bytes!("../assets/icons/update-download.svg"),
    ),
    (
        "update-restart",
        include_bytes!("../assets/icons/update-restart.svg"),
    ),
    ("edit", include_bytes!("../assets/icons/edit.svg")),
    (
        "image-rotate-left",
        include_bytes!("../assets/icons/image-rotate-left.svg"),
    ),
    (
        "image-rotate-right",
        include_bytes!("../assets/icons/image-rotate-right.svg"),
    ),
    (
        "image-flip-horizontal",
        include_bytes!("../assets/icons/image-flip-horizontal.svg"),
    ),
    (
        "image-flip-vertical",
        include_bytes!("../assets/icons/image-flip-vertical.svg"),
    ),
    (
        "image-crop",
        include_bytes!("../assets/icons/image-crop.svg"),
    ),
    (
        "image-reset",
        include_bytes!("../assets/icons/image-reset.svg"),
    ),
    (
        "image-save",
        include_bytes!("../assets/icons/image-save.svg"),
    ),
    (
        "file-upload",
        include_bytes!("../assets/icons/file-upload.svg"),
    ),
    (
        "file-download",
        include_bytes!("../assets/icons/file-download.svg"),
    ),
    ("connect", include_bytes!("../assets/icons/connect.svg")),
    (
        "disconnect",
        include_bytes!("../assets/icons/disconnect.svg"),
    ),
    ("server", include_bytes!("../assets/icons/server.svg")),
    ("check", include_bytes!("../assets/icons/check.svg")),
    ("shield", include_bytes!("../assets/icons/shield.svg")),
    (
        "folder-terminal",
        include_bytes!("../assets/icons/folder-terminal.svg"),
    ),
    ("back", include_bytes!("../assets/icons/back.svg")),
    ("up", include_bytes!("../assets/icons/up.svg")),
    ("down", include_bytes!("../assets/icons/down.svg")),
    ("copy", include_bytes!("../assets/icons/copy.svg")),
    ("save", include_bytes!("../assets/icons/save.svg")),
    ("reload", include_bytes!("../assets/icons/reload.svg")),
    ("wrap", include_bytes!("../assets/icons/wrap.svg")),
    ("maximize", include_bytes!("../assets/icons/maximize.svg")),
    ("restore", include_bytes!("../assets/icons/restore.svg")),
    ("monitor", include_bytes!("../assets/icons/monitor.svg")),
    ("plus", include_bytes!("../assets/icons/plus.svg")),
    ("settings", include_bytes!("../assets/icons/settings.svg")),
    ("close", include_bytes!("../assets/icons/close.svg")),
    ("chevron", include_bytes!("../assets/icons/chevron.svg")),
    ("eye", include_bytes!("../assets/icons/eye.svg")),
    ("eye-off", include_bytes!("../assets/icons/eye-off.svg")),
    ("folder", include_bytes!("../assets/icons/folder.svg")),
    ("pin", include_bytes!("../assets/icons/pin.svg")),
    ("settle", include_bytes!("../assets/icons/settle.svg")),
    ("unsettle", include_bytes!("../assets/icons/unsettle.svg")),
];
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }
    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS.iter().map(|(name, _)| (*name).into()).collect())
    }
}

pub(crate) fn icon(name: &'static str) -> gpui::Div {
    icon_sized(name, 16.)
}

pub(crate) fn icon_sized(name: &'static str, size: f32) -> gpui::Div {
    use gpui::prelude::*;
    gpui::div().size(gpui::px(size)).child(
        gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let color = window.text_style().color;
                if let Err(error) = window.paint_svg(
                    bounds,
                    name.into(),
                    gpui::TransformationMatrix::unit(),
                    color,
                    cx,
                ) {
                    eprintln!("Shellfox icon {name}: {error}");
                }
            },
        )
        .size_full(),
    )
}

pub(crate) fn icon_button(
    id: impl Into<gpui::SharedString>,
    name: &'static str,
    palette: crate::theme::Palette,
) -> gpui::Stateful<gpui::Div> {
    icon_button_sized(id, name, 28., 16., palette)
}

pub(crate) fn icon_button_sized(
    id: impl Into<gpui::SharedString>,
    name: &'static str,
    size: f32,
    icon_size: f32,
    palette: crate::theme::Palette,
) -> gpui::Stateful<gpui::Div> {
    use gpui::prelude::*;
    gpui::div()
        .id(id.into())
        .size(gpui::px(size))
        .rounded(gpui::px(4.))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_color(palette.muted)
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover).text_color(palette.accent))
        .child(icon_sized(name, icon_size))
}

pub(crate) fn tooltip(
    label: impl Into<gpui::SharedString>,
    _palette: crate::theme::Palette,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    let label = label.into();
    move |_, cx| {
        cx.new(|_| Tooltip {
            label: label.clone(),
            #[cfg(not(windows))]
            palette: _palette,
        })
        .into()
    }
}

struct Tooltip {
    label: gpui::SharedString,
    #[cfg(not(windows))]
    palette: crate::theme::Palette,
}

impl gpui::Render for Tooltip {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        #[cfg(windows)]
        {
            use gpui::{WindowAppearance, div, prelude::*, px, rgb};
            let dark = matches!(
                _window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            let (paper, ink) = if dark {
                (0x1f1f1f, 0xcacaca)
            } else {
                (0xffffff, 0x3c3c3c)
            };
            // Match the native title tooltip, not the renderer's immediate toolbar tip.
            div()
                .px(px(7.))
                .py(px(4.))
                .font_family(crate::ui_font::family())
                .text_size(px(12.))
                .line_height(px(15.))
                .bg(rgb(paper))
                .text_color(rgb(ink))
                .border_1()
                .border_color(rgb(ink))
                .whitespace_nowrap()
                .child(self.label.clone())
        }
        #[cfg(not(windows))]
        tooltip_label(self.label.clone(), self.palette)
    }
}

fn tooltip_label(label: SharedString, palette: crate::theme::Palette) -> gpui::Div {
    use gpui::prelude::*;
    use gpui::{div, px};
    div()
        .px(px(4.))
        .py(px(1.))
        .text_size(px(11.))
        .line_height(px(14.))
        .text_color(palette.foreground)
        .bg(palette.sidebar)
        .border_1()
        .border_color(palette.border)
        .whitespace_nowrap()
        .child(label)
}

pub(crate) fn toolbar_tip(
    button: gpui::Stateful<gpui::Div>,
    label: impl Into<SharedString>,
    palette: crate::theme::Palette,
) -> gpui::Stateful<gpui::Div> {
    use gpui::prelude::*;
    use gpui::px;
    button.relative().group("toolbar-tooltip").child(
        tooltip_label(label.into(), palette)
            .absolute()
            .top(px(30.))
            .right_0()
            .opacity(0.)
            .group_hover("toolbar-tooltip", |style| style.opacity(1.)),
    )
}

pub(crate) fn ellipsis(text: impl Into<SharedString>, line_height: f32) -> impl gpui::IntoElement {
    use gpui::prelude::*;
    use gpui::px;
    let text = text.into();
    // A flex item can be measured at its intrinsic width before it shrinks.
    // Use the final bounds so the ellipsis always fits the actual column.
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let style = window.text_style();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let system = window.text_system();
            let mut runs = vec![style.to_run(text.len())];
            let shortened = system.line_wrapper(style.font(), font_size).truncate_line(
                text.clone(),
                bounds.size.width,
                "…",
                &mut runs,
            );
            let line = system.shape_line(shortened, font_size, &runs, None);
            let _ = line.paint(bounds.origin, px(line_height), window, cx);
        },
    )
    .w_full()
    .h(px(line_height))
}

pub(crate) fn inline_ellipsis(text: impl Into<SharedString>, line_height: f32) -> impl gpui::IntoElement {
    use gpui::{div, prelude::*, px};
    let text = text.into();
    // Reserve the label's natural width so adjacent icons follow short labels.
    div()
        .relative()
        .h(px(line_height))
        .child(div().invisible().whitespace_nowrap().child(text.clone()))
        .child(div().absolute().inset_0().child(ellipsis(text, line_height)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_ui_icons_use_existing_svg_geometry_and_fixed_strokes() {
        for (name, bytes) in ICONS {
            let svg = std::str::from_utf8(bytes).unwrap();
            if name.starts_with("image-") {
                assert!(svg.contains("viewBox=\"0 0 24 24\""));
                assert!(svg.contains("stroke-width=\"1.7\""));
                continue;
            }
            if name.starts_with("file-") {
                assert!(svg.contains("viewBox=\"0 0 16 16\""));
                assert!(svg.contains("stroke-width=\"1.4\""));
                continue;
            }
            if name.starts_with("update-") {
                assert!(svg.contains("viewBox=\"0 0 24 24\""));
                assert!(svg.contains("stroke-width=\"1.5\""));
                continue;
            }
            assert!(svg.contains("viewBox=\"0 0 16 16\""));
            if ["save", "reload", "maximize", "restore", "wrap"].contains(name) {
                assert!(svg.contains("stroke-width=\"1.3\""));
            } else {
                assert!(svg.contains("stroke-width=\"1.25\""));
                assert!(svg.contains("stroke-linecap=\"round\""));
            }
        }
    }
}
