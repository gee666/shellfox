//! GPUI event routing for the actual remote owner view.
use super::*;
impl Render for PeerTerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .key_context("Terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                if let Some(action) = crate::input::clipboard_shortcut(&event.keystroke) {
                    view.clipboard_keys |= crate::input::clipboard_key_bit(&event.keystroke.key);
                    match action {
                        crate::input::ClipboardAction::Copy => view.copy(cx),
                        crate::input::ClipboardAction::Paste => {
                            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()).filter(|text| !text.is_empty()) {
                                view.admit(m::Input::Paste { text }, cx);
                            }
                        }
                    }
                    cx.stop_propagation();
                    return;
                }
                view.focus(window);
                if cfg!(target_os = "macos") && crate::input::mac_select_all(&event.keystroke) {
                    let _ = view.owner.set_selection(Some(shellfox_ghostty_vt::Selection::All));
                    cx.stop_propagation();
                    return;
                }
                let rows = view.screen.as_ref().map_or(0, |screen| screen.rows);
                if let Some(delta) = crate::input::page_scroll(&event.keystroke, rows) {
                    let _ = view.owner.scroll(delta as i32);
                    cx.stop_propagation();
                    return;
                }
                if view.preedit.is_empty() {
                    let action = if event.is_held { shellfox_ghostty_vt::KeyAction::Repeat } else { shellfox_ghostty_vt::KeyAction::Press };
                    if let Some(key) = crate::input::encode_physical(&event.keystroke, action) {
                        view.send_xterm_key(key, cx);
                        cx.stop_propagation();
                    }
                }
            }))
            .on_key_up(cx.listener(|view, event: &gpui::KeyUpEvent, _, cx| {
                let bit = crate::input::clipboard_key_bit(&event.keystroke.key);
                if view.clipboard_keys & bit != 0
                    || crate::input::clipboard_shortcut(&event.keystroke).is_some()
                {
                    view.clipboard_keys &= !bit;
                    cx.stop_propagation();
                    return;
                }

            }))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::down))
            .on_mouse_down(gpui::MouseButton::Middle, cx.listener(Self::down))
            .on_mouse_down(gpui::MouseButton::Right, cx.listener(Self::down))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::up))
            .on_mouse_up(gpui::MouseButton::Middle, cx.listener(Self::up))
            .on_mouse_up(gpui::MouseButton::Right, cx.listener(Self::up))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::up))
            .on_mouse_up_out(gpui::MouseButton::Middle, cx.listener(Self::up))
            .on_mouse_up_out(gpui::MouseButton::Right, cx.listener(Self::up))
            .on_mouse_move(cx.listener(Self::motion))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .child(div()
                .w_full().flex_1().min_h_0().flex().flex_col()
                .pl(px(renderer::HOST_INSET))
                .pr(px(renderer::HOST_RIGHT_INSET))
                .py(px(renderer::HOST_INSET))
                .child(surface::surface(cx.entity(), self.focus.clone())))
    }
}
