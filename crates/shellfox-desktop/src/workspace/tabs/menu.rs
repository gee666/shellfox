use super::reorder::TabStrip;
use crate::{root::Root, theme::Palette};
use gpui::{AnyElement, Context, FocusHandle, Pixels, Point, Window, div, prelude::*, px};
use shellfox_core::TabId;

pub(crate) struct TabMenu {
    strip: TabStrip,
    id: TabId,
    position: Point<Pixels>,
    previous_focus: Option<FocusHandle>,
    selected: usize,
    keyboard: bool,
}

impl TabMenu {
    pub(super) fn with_keyboard_focus(mut self) -> Self {
        self.keyboard = true;
        self
    }

    pub(super) fn new(strip: TabStrip, id: TabId, position: Point<Pixels>) -> Self {
        Self {
            strip,
            id,
            position,
            previous_focus: None,
            selected: 0,
            keyboard: false,
        }
    }
}

const LABELS: [&str; 3] = ["Rename terminal", "Move left", "Move right"];

impl Root {
    pub(super) fn open_tab_menu(
        &mut self,
        mut menu: TabMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_reorder_scope(cx) != Some(menu.strip) {
            return;
        }
        menu.previous_focus = if let Some(previous) = self.tabs.menu.take() {
            previous.previous_focus
        } else {
            window.focused(cx)
        };
        self.dialogs.modal_focus.focus(window);
        self.tabs.menu = Some(menu);
        cx.notify();
    }

    pub(crate) fn render_tab_menu(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(menu) = self.tabs.menu.as_ref() else {
            return div().into_any_element();
        };
        let enabled = self.tab_menu_actions(menu, cx);
        let viewport = window.viewport_size();
        let x = f32::from(menu.position.x).clamp(4., (f32::from(viewport.width) - 148.).max(4.));
        let y = f32::from(menu.position.y).clamp(4., (f32::from(viewport.height) - 98.).max(4.));
        let mut panel = div()
            .id("terminal-context-menu")
            .track_focus(&self.dialogs.modal_focus)
            .absolute()
            .occlude()
            .left(px(x))
            .top(px(y))
            .w(px(144.))
            .p(px(4.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .rounded(px(6.))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.4),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
            }])
            .on_mouse_down_out(cx.listener(|root, _, window, cx| {
                root.dismiss_tab_menu(window, cx);
            }))
            .on_key_down(
                cx.listener(move |root, event: &gpui::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => root.dismiss_tab_menu(window, cx),
                        "tab" => {
                            root.dismiss_tab_menu(window, cx);
                            root.move_strip_focus(event.keystroke.modifiers.shift, window, cx);
                        }
                        "enter" | "space" => {
                            if let Some(index) = root.tabs.menu.as_ref().map(|menu| menu.selected) {
                                root.run_tab_menu_action(index, window, cx);
                            }
                        }
                        key @ ("home" | "end") => {
                            let index = if key == "home" {
                                enabled.iter().position(|item| *item)
                            } else {
                                enabled.iter().rposition(|item| *item)
                            };
                            if let (Some(menu), Some(index)) = (&mut root.tabs.menu, index) {
                                menu.selected = index;
                                menu.keyboard = true;
                                cx.notify();
                            }
                        }
                        key @ ("up" | "down") => {
                            let Some(menu) = &mut root.tabs.menu else {
                                return;
                            };
                            menu.keyboard = true;
                            let step = if key == "up" { 2 } else { 1 };
                            for _ in 0..LABELS.len() {
                                menu.selected = (menu.selected + step) % LABELS.len();
                                if enabled[menu.selected] {
                                    break;
                                }
                            }
                            cx.notify();
                        }
                        _ => return,
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            );
        for (index, label) in LABELS.iter().enumerate() {
            panel = panel.child(
                div()
                    .id(("tab-menu-item", index))
                    .relative()
                    .w_full()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .text_size(px(12.))
                    .rounded(px(4.))
                    .text_color(palette.foreground)
                    .child(*label)
                    .when(enabled[index], |item| {
                        item.cursor_pointer()
                            .when(menu.keyboard && menu.selected == index, |item| {
                                item.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(palette.accent),
                                )
                            })
                            .hover(|style| style.bg(palette.hover).text_color(palette.foreground))
                            .on_click(cx.listener(move |root, _, window, cx| {
                                root.run_tab_menu_action(index, window, cx);
                            }))
                    })
                    .when(!enabled[index], |item| {
                        item.opacity(0.4).cursor(gpui::CursorStyle::Arrow)
                    }),
            );
        }
        panel.into_any_element()
    }

    fn tab_menu_actions(&self, menu: &TabMenu, cx: &Context<Self>) -> [bool; 3] {
        if self.tab_reorder_scope(cx) != Some(menu.strip) {
            return [false; 3];
        }
        [
            self.can_move_local_tab(menu.strip, menu.id, 0),
            self.can_move_local_tab(menu.strip, menu.id, -1),
            self.can_move_local_tab(menu.strip, menu.id, 1),
        ]
    }

    fn dismiss_tab_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.tabs.menu.take() {
            if self.dialogs.modal_focus.is_focused(window)
                && let Some(focus) = menu.previous_focus
            {
                focus.focus(window);
            }
            cx.notify();
        }
    }

    fn run_tab_menu_action(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.tabs.menu else {
            return;
        };
        if !self.tab_menu_actions(menu, cx)[index] {
            return;
        }
        let (strip, id) = (menu.strip, menu.id);
        self.dismiss_tab_menu(window, cx);
        match index {
            0 => self.rename_tab_dialog(id, window, cx),
            1 => self.move_local_tab(strip, id, -1, window, cx),
            2 => self.move_local_tab(strip, id, 1, window, cx),
            _ => unreachable!(),
        }
    }
}
