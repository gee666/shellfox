use super::scroll::TabFocus;
use crate::{
    accessibility::{Action, Role, SemanticAction, SemanticActionKind, SemanticNode},
    root::Root,
    theme::Palette,
};
use gpui::{AnyElement, Context, Div, KeyDownEvent, MouseButton, Stateful, Window, prelude::*};

pub(super) fn files_shortcut_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Files (⌘⇧B)"
    } else {
        "Files (Ctrl+Shift+B)"
    }
}

impl TabFocus {
    fn semantic_key(self) -> Option<String> {
        Some(match self {
            Self::Label(_) => return None,
            Self::Close(target) => format!(
                "workspace/terminal-tabs/close/{}/{}",
                target.tab_id, target.generation
            ),
            Self::NewTerminal => "workspace/terminal-tabs/new".into(),
            Self::Files => "workspace/terminal-tabs/files".into(),
            Self::Folder => "workspace/terminal-tabs/folder".into(),
        })
    }
}

impl Root {
    fn files_pressed(&self) -> bool {
        self.files_source_current() && self.files.panel.as_ref().is_some_and(|files| files.visible)
    }

    pub(crate) fn render_files_toggle(
        &self,
        available: bool,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let key = TabFocus::Files;
        let button = crate::assets::icon_button("toggle-files", "folder", palette)
            .tooltip(crate::assets::tooltip(files_shortcut_hint(), palette))
            .when(self.files_pressed(), |button| {
                button.bg(gpui::rgb(0x333333))
            })
            .when(!available, |button| button.opacity(0.4));
        let focus = available.then(|| self.tabs.scroll.control_focus(key, cx).tab_stop(true));
        self.tab_strip_control(key, "Files", button, focus, palette, cx)
    }

    pub(super) fn tab_strip_control(
        &self,
        key: TabFocus,
        name: impl Into<String>,
        button: Stateful<Div>,
        focus: Option<gpui::FocusHandle>,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let files_control = matches!(key, TabFocus::Files);
        let show_outline = !files_control || !self.tabs.scroll.files_pointer_focus.get();
        let button = button.when_some(focus.clone(), |button, focus| {
            button
                .track_focus(&focus)
                .when(show_outline, |button| {
                    button.focus(|style| style.opacity(1.).border_1().border_color(palette.accent))
                })
                .when(files_control, |button| {
                    let pointer_focus = focus.clone();
                    button.capture_any_mouse_down(cx.listener(
                        move |root, event: &gpui::MouseDownEvent, window, cx| {
                            if matches!(event.button, MouseButton::Left | MouseButton::Right)
                                && !pointer_focus.is_focused(window)
                            {
                                root.tabs.scroll.files_pointer_focus.set(true);
                                cx.notify();
                            }
                        },
                    ))
                })
                .on_mouse_down(MouseButton::Right, move |_, window, _| focus.focus(window))
                .on_click(cx.listener(move |root, _, window, cx| {
                    root.activate_tab_control(key, window, cx)
                }))
                .on_key_down(cx.listener(move |root, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        root.activate_tab_control(key, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    } else if matches!(key, TabFocus::Files) {
                        root.tab_traversal(event, window, cx);
                    }
                }))
        });
        let Some(key) = key.semantic_key() else {
            return button.into_any_element();
        };
        let frame = self.semantics.context.frame();
        match frame.id_for(&key) {
            Ok(id) => {
                let mut node = SemanticNode::new(id, Some(frame.root_id()), Role::Button, name);
                if files_control {
                    let open = self.files_pressed();
                    node.data.set_toggled(if open {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    });
                    node.data.set_description(files_shortcut_hint());
                }
                if focus.is_some() {
                    node.data.add_action(Action::Click);
                } else {
                    node.data.set_disabled();
                }
                node.focus = focus;
                frame.wrap(node, button).into_any_element()
            }
            Err(error) => {
                frame.reject(error);
                button.into_any_element()
            }
        }
    }

    pub(super) fn tab_traversal(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mods = event.keystroke.modifiers;
        if event.keystroke.key == "tab" && !mods.control && !mods.alt && !mods.platform {
            self.move_strip_focus(mods.shift, window, cx);
            window.prevent_default();
            cx.stop_propagation();
        }
    }

    pub(crate) fn move_strip_focus(
        &self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tabs.scroll.files_pointer_focus.set(false);
        if reverse {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        if let Some(tab) = self.tabs.scroll.focused_tab(window) {
            self.tabs.scroll.reveal_on_paint(tab);
        }
        cx.notify();
    }

    pub(crate) fn focus_files_toggle(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.scroll.files_pointer_focus.set(false);
        self.tabs
            .scroll
            .control_focus(TabFocus::Files, cx)
            .focus(window);
        cx.notify();
    }

    fn activate_tab_control(&mut self, key: TabFocus, window: &mut Window, cx: &mut Context<Self>) {
        match key {
            TabFocus::Close(target) => self.request_tab_close(target, window, cx),
            TabFocus::NewTerminal => {
                if let Some(session) = self.selected_session() {
                    self.add_tab(session, window, cx);
                }
            }
            TabFocus::Files => self.toggle_files_layout(window, cx),
            TabFocus::Folder => self.open_terminal_folder(cx),
            TabFocus::Label(_) => {}
        }
    }

    pub(crate) fn handle_tab_control_semantic_action(
        &mut self,
        action: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(
            action.kind,
            SemanticActionKind::Click | SemanticActionKind::Focus
        ) || self.settings.open
            || self.dialogs.current.is_some()
            || self.tabs.menu.is_some()
            || self.tabs.profile_menu.is_some()
        {
            return false;
        }
        let frame = self.semantics.context.frame();
        if frame.focus_handle(action.target).is_none() {
            return false;
        }
        let key = self.tabs.scroll.controls().into_iter().find(|control| {
            control
                .semantic_key()
                .and_then(|key| frame.id_for(&key).ok())
                == Some(action.target)
        });
        if let Some(key) = key {
            match (&action.kind, key) {
                (SemanticActionKind::Click, _) => self.activate_tab_control(key, window, cx),
                (SemanticActionKind::Focus, TabFocus::Files) => self.focus_files_toggle(window, cx),
                _ => return false,
            }
            return true;
        }
        false
    }
}
