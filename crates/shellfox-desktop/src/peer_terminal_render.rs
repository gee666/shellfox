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
                let modifiers = event.keystroke.modifiers;
                if (modifiers.control && modifiers.shift || modifiers.platform)
                    && event.keystroke.key == "c"
                {
                    view.copy(cx);
                    cx.stop_propagation();
                    return;
                }
                if modifiers.control && modifiers.shift && event.keystroke.key == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        view.admit(m::Input::Paste { text }, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                if modifiers.platform && event.keystroke.key == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        view.admit(m::Input::Paste { text }, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                view.focus(window);
                if let Some(event) = keys::physical(
                    &event.keystroke,
                    if event.is_held {
                        m::KeyActionV2::Repeat
                    } else {
                        m::KeyActionV2::Press
                    },
                    !view.preedit.is_empty(),
                ) {
                    view.admit(m::Input::Key { event }, cx);
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(|view, event: &gpui::KeyUpEvent, _, cx| {
                if let Some(event) = keys::physical(
                    &event.keystroke,
                    m::KeyActionV2::Release,
                    !view.preedit.is_empty(),
                ) {
                    view.admit(m::Input::Key { event }, cx);
                    cx.stop_propagation();
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
            .child(surface::surface(cx.entity(), self.focus.clone()))
    }
}
