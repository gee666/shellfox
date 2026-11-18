use crate::{root::Root, theme::Palette};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
impl Root {
    pub(crate) fn workspace_width(&self, window: &Window) -> f32 {
        f32::from(window.viewport_size().width)
            - if self.sidebar {
                crate::sidebar::clamp_width(self.sidebar_width, f32::from(window.viewport_size().width)) + 4.
            } else {
                28.
            }
    }
    pub(crate) fn files_narrow(&self, window: &Window) -> bool {
        f32::from(window.viewport_size().width) <= 700. || self.workspace_width(window) < 500.
    }
    pub(crate) fn resize_layout(
        &mut self,
        event: &gpui::MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let left = event.pressed_button == Some(gpui::MouseButton::Left);
        let mut changed = self.resizing_sidebar;
        if self.resizing_sidebar {
            if left {
                self.sidebar_width = crate::sidebar::clamp_width(
                    f32::from(event.position.x),
                    f32::from(window.viewport_size().width),
                );
            } else {
                self.resizing_sidebar = false;
            }
        }
        let workspace = self.workspace_width(window);
        if let Some(files) = &mut self.files {
            if files.resizing_width {
                changed = true;
                if left {
                    let max = (workspace - 200.).max(0.);
                    files.width = (f32::from(window.viewport_size().width)
                        - f32::from(event.position.x))
                    .clamp(300f32.min(max), max);
                } else {
                    files.resizing_width = false;
                }
            }
            if files.resizing_browser {
                changed = true;
                if left {
                    let max = (f32::from(window.viewport_size().height) - 200.).max(0.);
                    files.browser_height =
                        (f32::from(event.position.y) - 32.).clamp(68f32.min(max), max);
                } else {
                    files.resizing_browser = false;
                }
            }
        }
        if changed {
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn layout_body(
        &self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = div().flex().size_full();
        if self.sidebar {
            content = content.child(self.session_sidebar(palette, window, cx)).child(
                div()
                    .id("sidebar-resizer")
                    .w(px(4.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(palette.sidebar)
                    .border_r_1()
                    .border_color(palette.border)
                    .cursor(gpui::CursorStyle::ResizeColumn)
                    .hover(|style| style.bg(palette.accent))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            root.resizing_sidebar = true;
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            root.resizing_sidebar = false;
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        gpui::MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            root.resizing_sidebar = false;
                            cx.notify();
                        }),
                    ),
            );
        }
        let mut workspace = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        if !self.sidebar {
            content = content.child(
                div()
                    .w(px(28.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(palette.sidebar)
                    .border_r_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .h(px(32.))
                            .px(px(2.))
                            .flex()
                            .items_center()
                            .border_b_1()
                            .border_color(palette.border)
                            .child(
                                crate::assets::icon_button("show-sessions", "chevron", palette)
                                    .w(px(24.))
                                    .on_click(cx.listener(|root, _, _, cx| {
                                        root.sidebar = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        workspace = workspace.child(if self.settings {
            let child = self.settings_page(palette, cx);
            // Settings is an uncached view. Every paint measures its current semantic nodes.
            if self.settings_panel.is_none() {
                self.semantic_region(
                    "settings/panel",
                    crate::accessibility::Role::Pane,
                    "Settings",
                    child,
                )
            } else {
                child
            }
        } else {
            self.terminal_workspace(window, palette, cx)
        });
        content = content.child(workspace);
        content.into_any_element()
    }
}
