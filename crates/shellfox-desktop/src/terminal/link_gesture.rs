use super::{TerminalView, link_menu::LinkMenuPress, links::copy_link};
use gpui::Context;
use shellfox_ghostty_vt::ViewportPoint;
use shellfox_ghostty_vt::terminal_links::parse_file_link;

pub(super) struct FilePress {
    pub start: gpui::Point<gpui::Pixels>,
    pub target: String,
    pub modifiers: gpui::Modifiers,
    pub moved: bool,
}

impl FilePress {
    fn moved_to(&self, position: gpui::Point<gpui::Pixels>) -> bool {
        let delta = position - self.start;
        self.moved || f32::from(delta.x).hypot(f32::from(delta.y)) > 4.
    }
}

impl TerminalView {
    pub(super) fn link_pointer_down(
        &mut self,
        point: ViewportPoint,
        event: &gpui::MouseDownEvent,
        context_click: bool,
    ) -> bool {
        if context_click && let Some(link) = self.visible_link_at(point) {
            self.links.menu_press = Some(LinkMenuPress {
                button: event.button,
                target: link.target,
            });
            return true;
        }
        self.links.file_press = self.file_link_press(point, event);
        self.links.file_press.is_some()
    }

    pub(super) fn link_pointer_up(
        &mut self,
        event: &gpui::MouseUpEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button == gpui::MouseButton::Left
            && let Some(press) = self.links.file_press.take()
        {
            if !press.moved_to(event.position) {
                self.open_link(Some(press.target), press.modifiers, cx);
            }
            return true;
        }
        if self
            .links
            .menu_press
            .as_ref()
            .is_some_and(|press| press.button == event.button)
        {
            if let Some(press) = self.links.menu_press.take() {
                self.open_link_menu(press.target, event.position, cx);
            }
            return true;
        }
        false
    }

    pub(super) fn file_link_motion(&mut self, position: gpui::Point<gpui::Pixels>) -> bool {
        let Some(press) = self.links.file_press.as_mut() else {
            return false;
        };
        press.moved = press.moved_to(position);
        true
    }

    fn file_link_press(
        &self,
        point: ViewportPoint,
        event: &gpui::MouseDownEvent,
    ) -> Option<FilePress> {
        if !self.links.files_enabled
            || event.button != gpui::MouseButton::Left
            || cfg!(target_os = "macos") && event.modifiers.control
        {
            return None;
        }
        let mods = event.modifiers;
        let force_selection = if cfg!(target_os = "macos") {
            mods.alt
        } else {
            mods.shift
        };
        if force_selection && !copy_link(mods) {
            return None;
        }
        let target = self.visible_link_at(point)?.target;
        parse_file_link(&target)?;
        Some(FilePress {
            start: event.position,
            target,
            modifiers: mods,
            moved: false,
        })
    }
}
