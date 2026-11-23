use super::panes::PaneKind;
use crate::{root::Root, theme::Palette};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
pub(crate) fn sessions_tooltip(expanded: bool) -> &'static str {
    match (expanded, cfg!(target_os = "macos")) {
        (true, true) => "Hide sessions (⌘B)",
        (false, true) => "Show sessions (⌘B)",
        (true, false) => "Hide sessions (Ctrl+B)",
        (false, false) => "Show sessions (Ctrl+B)",
    }
}

impl Root {
    pub(crate) fn workspace_width(&self, window: &Window) -> f32 {
        f32::from(window.viewport_size().width)
            - if self.sidebar.visible {
                crate::sidebar::clamp_width(
                    self.sidebar.width,
                    f32::from(window.viewport_size().width),
                ) + 4.
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
        if self.resize_session_environment(event, window, cx) {
            return;
        }
        let left = event.pressed_button == Some(gpui::MouseButton::Left);
        let mut changed = self.sidebar.resizing;
        if self.sidebar.resizing {
            if left && let Some(start) = self.sidebar.resize_origin {
                self.sidebar.width = start.width(
                    f32::from(event.position.x),
                    f32::from(window.viewport_size().width),
                );
            } else {
                self.sidebar.resizing = false;
                self.sidebar.resize_origin = None;
            }
        }
        changed |= self.resize_files(event, window, cx);
        if changed {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn sidebar_resize_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = if event.keystroke.modifiers.shift {
            32.
        } else {
            4.
        };
        let width = match event.keystroke.key.as_str() {
            "left" => self.sidebar.width - step,
            "right" => self.sidebar.width + step,
            "home" => 220.,
            _ => return,
        };
        self.sidebar.width =
            crate::sidebar::clamp_width(width, f32::from(window.viewport_size().width));
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn layout_body(
        &self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = div().flex().size_full();
        if self.sidebar.visible {
            let width = crate::sidebar::clamp_width(
                self.sidebar.width,
                f32::from(window.viewport_size().width),
            );
            content = content
                .child(
                    div()
                        .w(px(width))
                        .flex_shrink_0()
                        .h_full()
                        .child(self.pane(PaneKind::Sidebar, cx)),
                )
                .child(
                    div()
                        .id("sidebar-resizer")
                        .track_focus(&self.sidebar.resizer)
                        .on_key_down(cx.listener(Self::sidebar_resize_key))
                        .w(px(4.))
                        .h_full()
                        .flex_shrink_0()
                        .bg(palette.sidebar)
                        .border_r_1()
                        .border_color(palette.border)
                        .cursor(gpui::CursorStyle::ResizeColumn)
                        .hover(|style| style.bg(palette.accent))
                        .focus(|style| style.border_1().border_color(palette.accent))
                        .when(self.sidebar.resizing, |style| style.bg(palette.accent))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|root, event: &gpui::MouseDownEvent, window, cx| {
                                root.sidebar.resizer.focus(window);
                                let viewport = f32::from(window.viewport_size().width);
                                if event.click_count > 1 {
                                    root.sidebar.width =
                                        crate::sidebar::clamp_width(220., viewport);
                                    root.sidebar.resizing = false;
                                    root.sidebar.resize_origin = None;
                                } else {
                                    root.sidebar.resize_origin =
                                        Some(crate::sidebar::SidebarDrag::start(
                                            f32::from(event.position.x),
                                            root.sidebar.width,
                                            viewport,
                                        ));
                                    root.sidebar.resizing = true;
                                }
                                cx.notify();
                                cx.stop_propagation();
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(|root, _, _, cx| {
                                root.sidebar.resizing = false;
                                root.sidebar.resize_origin = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_up_out(
                            gpui::MouseButton::Left,
                            cx.listener(|root, _, _, cx| {
                                root.sidebar.resizing = false;
                                root.sidebar.resize_origin = None;
                                cx.notify();
                            }),
                        ),
                );
        }
        let mut workspace = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        if !self.sidebar.visible {
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
                                    .track_focus(&self.sidebar.toggle)
                                    .focus(|style| style.border_1().border_color(palette.accent))
                                    .tooltip(crate::assets::tooltip(
                                        sessions_tooltip(false),
                                        palette,
                                    ))
                                    .on_click(cx.listener(|root, _, window, cx| {
                                        root.sidebar.toggle.focus(window);
                                        root.toggle_sessions_layout(window, cx);
                                    })),
                            ),
                    ),
            );
        }
        workspace = workspace.child(if self.settings.open {
            let child = self.settings_page(palette, cx);
            if self.settings.panel.is_none() {
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
