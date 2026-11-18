use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;
const ICONS: &[(&str, &[u8])] = &[
    ("more", include_bytes!("../assets/icons/more.svg")),
    ("edit", include_bytes!("../assets/icons/edit.svg")),
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
    use gpui::prelude::*;
    gpui::div().size(gpui::px(16.)).child(
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
    id: &'static str,
    name: &'static str,
    palette: crate::theme::Palette,
) -> gpui::Stateful<gpui::Div> {
    use gpui::prelude::*;
    gpui::div()
        .id(id)
        .w(gpui::px(28.))
        .h(gpui::px(28.))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_color(palette.muted)
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover).text_color(palette.accent))
        .child(icon(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_ui_icons_use_existing_svg_geometry_and_fixed_strokes() {
        for (name, bytes) in ICONS {
            let svg = std::str::from_utf8(bytes).unwrap();
            assert!(svg.contains("viewBox=\"0 0 16 16\""));
            if ["save", "reload", "maximize", "restore"].contains(name) {
                assert!(svg.contains("stroke-width=\"1.3\""));
            } else {
                assert!(svg.contains("stroke-width=\"1.25\""));
                assert!(svg.contains("stroke-linecap=\"round\""));
            }
        }
    }
}
