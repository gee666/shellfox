use super::*;
use gpui::KeyUpEvent;

impl Root {
    pub(super) fn session_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.sidebar.session_menu else {
            return;
        };
        let items = self
            .menu_session(&menu.target, cx)
            .map(|info| info.items())
            .unwrap_or_default();
        let key = event.keystroke.key.as_str();
        if matches!(key, "escape" | "tab") {
            self.dismiss_session_menu(window, cx);
        } else if !items.is_empty()
            && let Some(menu) = self.sidebar.session_menu.as_mut()
        {
            match key {
                "up" | "down" | "home" | "end" => {
                    menu.show_focus = true;
                    menu.selected = match key {
                        "up" => (menu.selected + items.len() - 1) % items.len(),
                        "down" => (menu.selected + 1) % items.len(),
                        "home" => 0,
                        _ => items.len() - 1,
                    };
                }
                "space" | "enter" => {}
                _ => return,
            }
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn session_menu_activate(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event.keystroke.key.as_str(), "enter" | "space") {
            return;
        }
        let Some(menu) = &self.sidebar.session_menu else {
            return;
        };
        let Some(info) = self.menu_session(&menu.target, cx) else {
            return;
        };
        if let Some((item, _)) = info.items().get(menu.selected) {
            self.sidebar.keyboard_focus = true;
            self.run_session_item(*item, window, cx);
            window.prevent_default();
            cx.stop_propagation();
        }
    }
}
