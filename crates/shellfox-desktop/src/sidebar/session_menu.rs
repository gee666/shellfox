use crate::{
    peers::source::PeerCycle,
    root::{Dialog, Root},
    theme::Palette,
    workspace::AfterAction,
};
use gpui::{
    AnyElement, Context, FocusHandle, KeyDownEvent, Pixels, Point, Window, div, prelude::*, px,
};
use shellfox_core::{Session, SessionId};
use shellfox_service::native::NativeAction;

mod delete;
mod keys;

#[derive(Clone)]
enum Target {
    Local(SessionId),
    Peer(PeerCycle, SessionId),
}

pub(crate) struct SessionMenu {
    target: Target,
    position: Point<Pixels>,
    selected: usize,
    show_focus: bool,
    prior: Option<FocusHandle>,
}
#[derive(Clone, Copy)]
pub(crate) enum Item {
    Pin,
    Rename,
    Environment,
    Explorer,
    Copy,
    ClearError,
    Settle,
    Delete,
}

pub(crate) struct MenuSession {
    pub pinned: bool,
    pub settled: bool,
    pub has_error: bool,
    pub pending: bool,
    pub local: bool,
}
impl MenuSession {
    fn items(&self) -> Vec<(Item, &'static str)> {
        let mut items = vec![
            (Item::Pin, if self.pinned { "Unpin" } else { "Pin to top" }),
            (Item::Rename, "Rename"),
            (Item::Environment, "Environment variables…"),
        ];
        if self.local {
            items.push((Item::Explorer, "Open in File Explorer"));
        }
        items.push((Item::Copy, "Copy path"));
        if self.has_error {
            items.push((Item::ClearError, "Clear error"));
        }
        items.extend([
            (
                Item::Settle,
                if self.settled { "Unsettle" } else { "Settle" },
            ),
            (Item::Delete, "Delete…"),
        ]);
        items
    }
}

impl Root {
    pub(crate) fn open_session_menu(
        &mut self,
        id: SessionId,
        at: Point<Pixels>,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_context_session(Target::Local(id), at, keyboard, window, cx);
    }

    pub(crate) fn open_peer_session_menu(
        &mut self,
        cycle: PeerCycle,
        id: SessionId,
        at: Point<Pixels>,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_context_session(Target::Peer(cycle, id), at, keyboard, window, cx);
    }

    fn open_context_session(
        &mut self,
        target: Target,
        position: Point<Pixels>,
        show_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window || self.dialog_locked() {
            return;
        }
        let prior = self
            .session_menu
            .take()
            .and_then(|menu| menu.prior)
            .or_else(|| window.focused(cx));
        self.modal_focus.focus(window);
        self.session_menu = Some(SessionMenu {
            target,
            position,
            selected: 0,
            show_focus,
            prior,
        });
        cx.notify();
    }

    fn dismiss_session_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.session_menu.take() {
            if self.modal_focus.is_focused(window)
                && let Some(focus) = menu.prior
            {
                focus.focus(window);
            }
            cx.notify();
        }
    }

    fn session_by_id(&self, id: SessionId) -> Option<&Session> {
        self.snapshot
            .as_ref()?
            .sessions
            .iter()
            .find(|session| session.id == id)
    }

    fn menu_session(&self, target: &Target, cx: &gpui::App) -> Option<MenuSession> {
        match target {
            Target::Peer(cycle, id) => self.peer_session_menu_info(cycle, *id),
            Target::Local(id) => {
                let session = self.session_by_id(*id)?;
                Some(MenuSession {
                    pinned: session.pinned_at.is_some(),
                    settled: session.settled_at.is_some(),
                    has_error: self.session_has_error(session, cx),
                    pending: self.user_action_pending,
                    local: true,
                })
            }
        }
    }

    pub(crate) fn render_session_menu(
        &self,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(menu) = &self.session_menu else {
            return div().into_any_element();
        };
        let Some(info) = self.menu_session(&menu.target, cx) else {
            return div().into_any_element();
        };
        let items = info.items();
        let width = if info.local { 150. } else { 148. };
        let viewport = window.viewport_size();
        let x =
            f32::from(menu.position.x).clamp(4., (f32::from(viewport.width) - width - 4.).max(4.));
        let y = f32::from(menu.position.y).clamp(
            4.,
            (f32::from(viewport.height) - (14. + 28. * items.len() as f32)).max(4.),
        );
        let mut panel = div()
            .id("session-context-menu")
            .track_focus(&self.modal_focus)
            .absolute()
            .occlude()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .p(px(4.))
            .rounded(px(6.))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.border)
            .shadow_lg()
            .on_mouse_down_out(
                cx.listener(|root, _, window, cx| root.dismiss_session_menu(window, cx)),
            )
            .on_key_down(cx.listener(Self::session_menu_key))
            .on_key_up(cx.listener(Self::session_menu_activate));
        for (index, (item, label)) in items.into_iter().enumerate() {
            panel = panel.child(
                div()
                    .id(("session-menu-item", index))
                    .w_full()
                    .h(px(28.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .text_size(px(12.))
                    .child(label)
                    .when(!info.pending, |button| {
                        button
                            .cursor_pointer()
                            .when(menu.show_focus && menu.selected == index, |button| {
                                button.border_1().border_color(palette.accent).pl(px(7.))
                            })
                            .hover(|style| style.bg(palette.hover))
                            .on_click(cx.listener(move |root, _, window, cx| {
                                root.run_session_item(item, window, cx)
                            }))
                    })
                    .when(info.pending, |button| button.opacity(0.4)),
            );
        }
        panel.into_any_element()
    }

    fn run_session_item(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.session_menu.as_ref().map(|menu| menu.target.clone()) else {
            return;
        };
        let Some(info) = self.menu_session(&target, cx) else {
            return;
        };
        if info.pending || self.closing_window {
            return;
        }
        self.dismiss_session_menu(window, cx);
        let id = match target {
            Target::Peer(cycle, id) => {
                return self.run_peer_session_item(cycle, id, item, window, cx);
            }
            Target::Local(id) => id,
        };
        match item {
            Item::Pin => self.toggle_session_pinned(id, !info.pinned, window, cx),
            Item::Rename => self.rename_session_dialog(id, window, cx),
            Item::Environment => self.edit_session_environment(id, window, cx),
            Item::Explorer => {
                if let Some(cwd) = self.session_by_id(id).map(|session| session.cwd.clone()) {
                    self.open_session_folder(cwd, cx);
                }
            }
            Item::Copy => {
                if let Some(cwd) = self.session_by_id(id).map(|session| session.cwd.clone()) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(cwd));
                    self.notify_info(cx, "Path copied");
                }
            }
            Item::ClearError => self.submit_sidebar_action(
                NativeAction::ClearSessionError { session: id },
                AfterAction::Mutation,
                window,
                cx,
            ),
            Item::Settle => self.toggle_session_settled(id, !info.settled, window, cx),
            Item::Delete => {
                self.modal_focus.focus(window);
                self.dialog = Some(Dialog::DeleteSession(id));
            }
        }
        cx.notify();
    }

    fn open_session_folder(&mut self, cwd: String, cx: &mut Context<Self>) {
        use shellfox_service::system::{NativeSystemOpener, SystemOpenTarget, SystemOpener};
        let task = cx.background_executor().spawn(async move {
            let cwd = crate::dialogs::directory::validate_session_folder(&cwd)?;
            NativeSystemOpener {
                interactive_session: true,
            }
            .open(SystemOpenTarget::Directory(cwd))
            .map_err(|error| format!("The session folder could not be opened: {}", error.message))
        });
        cx.spawn(async move |root, cx| {
            if let Err(message) = task.await {
                let _ = root.update(cx, |root, cx| {
                    root.error = Some(message);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
