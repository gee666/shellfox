use crate::{assets::icon_button, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

impl Root {
    pub(super) fn linux_titlebar(
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if window.is_fullscreen()
            || !matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            )
        {
            return None;
        }
        let controls = window.window_controls();
        let caption = div()
            .id("window-caption")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .px(px(12.))
            .on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
                if event.click_count > 1 && window.window_controls().maximize {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            })
            .on_mouse_down(gpui::MouseButton::Right, |event, window, _| {
                if window.window_controls().window_menu {
                    window.show_window_menu(event.position);
                }
            })
            .child(div().truncate().child(crate::desktop_windows::caption(cx)));
        let mut titlebar = div()
            .h(px(30.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .px(px(2.))
            .bg(palette.sidebar)
            .border_b_1()
            .border_color(palette.border)
            .text_size(px(12.))
            .child(caption);
        if controls.minimize {
            titlebar = titlebar.child(
                super::button("minimize-window", "−", palette)
                    .w(px(28.))
                    .on_click(|_, window, _| window.minimize_window()),
            );
        }
        if controls.maximize {
            titlebar = titlebar.child(
                icon_button("maximize-window", "maximize", palette)
                    .on_click(|_, window, _| window.zoom_window()),
            );
        }
        Some(
            titlebar
                .child(
                    icon_button("close-window", "close", palette).on_click(cx.listener(
                        |root, _, window, cx| {
                            if root.close(window, cx) {
                                window.remove_window();
                            }
                        },
                    )),
                )
                .into_any_element(),
        )
    }
}
