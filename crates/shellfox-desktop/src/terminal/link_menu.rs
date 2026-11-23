use super::TerminalView;
use crate::theme::Palette;
use gpui::{
    AnyElement, ClipboardItem, Context, MouseButton, Pixels, Point, Window, div, prelude::*, px,
};

const LABELS: [&str; 2] = ["Open link", "Copy link address"];
// Electron's .context-menu in styles.css.
const MENU_RADIUS: f32 = 6.;
const MENU_CSS_BLUR_RADIUS: f32 = 24.;

fn menu_shadow(dark: bool) -> gpui::BoxShadow {
    gpui::BoxShadow {
        // Electron's --shadow in theme.ts.
        color: gpui::hsla(0., 0., 0., if dark { 0.4 } else { 0.18 }),
        offset: gpui::point(px(0.), px(8.)),
        // GPUI takes the Gaussian standard deviation, half the CSS blur radius.
        blur_radius: px(MENU_CSS_BLUR_RADIUS / 2.),
        spread_radius: px(0.),
    }
}

pub(super) struct LinkMenu {
    target: String,
    position: Point<Pixels>,
    selected: usize,
}

pub(super) struct LinkMenuPress {
    pub button: MouseButton,
    pub target: String,
}

impl TerminalView {
    pub(super) fn open_link_menu(
        &mut self,
        target: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.links.menu = Some(LinkMenu {
            target,
            position,
            selected: 0,
        });
        cx.notify();
    }

    fn close_link_menu(&mut self, cx: &mut Context<Self>) {
        self.links.menu = None;
        cx.notify();
    }

    fn use_link_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.links.menu.take() else {
            return;
        };
        match index {
            0 => self.open_link(Some(menu.target), gpui::Modifiers::default(), cx),
            1 => cx.write_to_clipboard(ClipboardItem::new_string(menu.target)),
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn link_menu_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = &mut self.links.menu else {
            return false;
        };
        match event.keystroke.key.as_str() {
            "escape" => self.close_link_menu(cx),
            "tab" => {
                self.close_link_menu(cx);
                if event.keystroke.modifiers.shift {
                    window.focus_prev();
                } else {
                    window.focus_next();
                }
            }
            "enter" | "space" => {
                let index = menu.selected;
                self.use_link_menu(index, cx);
            }
            "up" | "down" | "home" | "end" => {
                menu.selected = match event.keystroke.key.as_str() {
                    "home" => 0,
                    "end" => 1,
                    _ => 1 - menu.selected,
                };
                cx.notify();
            }
            _ => {}
        }
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(super) fn render_link_menu(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let menu = self.links.menu.as_ref()?;
        let palette = Palette::new(
            &format!("#{:06x}", self.theme.accent),
            &format!("#{:06x}", self.theme.background),
        );
        let mut panel = div()
            .id("terminal-link-menu")
            .occlude()
            .w(px(144.))
            .p(px(4.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .rounded(px(MENU_RADIUS))
            .font_family(crate::ui_font::family())
            .text_size(px(12.))
            .text_color(palette.foreground)
            .shadow(vec![menu_shadow(palette.dark)])
            .on_mouse_down_out(cx.listener(|view, _, _, cx| view.close_link_menu(cx)));
        for (index, label) in LABELS.into_iter().enumerate() {
            panel = panel.child(
                div()
                    .id(("terminal-link-action", index))
                    .relative()
                    .w_full()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .cursor_pointer()
                    .rounded(px(4.))
                    .hover(move |item| item.bg(palette.hover))
                    .when(menu.selected == index, |item| {
                        item.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(palette.accent),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.use_link_menu(index, cx);
                        cx.stop_propagation();
                    }))
                    .child(label),
            );
        }
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(4.))
                    .child(panel),
            )
            .with_priority(42)
            .into_any_element(),
        )
    }
}
