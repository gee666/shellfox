use super::{Backend, FIXTURE, SharedScreen, TerminalTheme, TerminalView, runtime};
use crate::{host_client::HostClient, options::Mode, renderer};
use anyhow::Result;
use gpui::{App, Context, Entity, Window, prelude::*};
use runtime::native::{Access, Connection};
use shellfox_core::TabId;
use shellfox_ghostty_vt::Terminal;
use shellfox_service::{Attachment, native::NativeObserverAttachment};
use std::{collections::HashMap, path::Path, rc::Rc};

impl TerminalView {
    pub fn open(
        mode: Mode,
        _: &Path,
        window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<Entity<Self>, crate::root::StartupError> {
        if mode != Mode::Fixture {
            return Err(crate::root::StartupError::from(anyhow::anyhow!(
                "Attach to the application service before creating a shell view"
            )));
        }
        Self::open_inner(None, None, window, cx).map_err(crate::root::StartupError::from)
    }
    pub fn open_attached(
        client: HostClient,
        tab: TabId,
        attachment: Attachment,
        window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<Entity<Self>, crate::root::StartupError> {
        Self::open_inner(
            Some((client, tab, Access::Control(attachment))),
            None,
            window,
            cx,
        )
        .map_err(crate::root::StartupError::from)
    }
    pub fn open_observed(
        client: HostClient,
        tab: TabId,
        attachment: NativeObserverAttachment,
        window: &mut Window,
        cx: &mut App,
    ) -> std::result::Result<Entity<Self>, crate::root::StartupError> {
        Self::open_inner(
            Some((client, tab, Access::Observer(attachment))),
            None,
            window,
            cx,
        )
        .map_err(crate::root::StartupError::from)
    }
    pub(crate) fn open_peer(
        binding: std::sync::Arc<crate::host_client::peer_v1::V1Binding>,
        tab: TabId,
        frontend_replies: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Entity<Self>> {
        Self::open_inner(None, Some((binding, tab, frontend_replies)), window, cx)
    }

    fn open_inner(
        attached: Option<(HostClient, TabId, Access)>,
        peer: Option<(
            std::sync::Arc<crate::host_client::peer_v1::V1Binding>,
            TabId,
            bool,
        )>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Entity<Self>> {
        let (font, cell_width, line_height) =
            renderer::choose_font(window, "Cascadia Code", renderer::FONT_SIZE)?;
        let theme = TerminalTheme::default();
        let (backend, screen, status, incoming, lease) =
            if let Some((client, tab, access)) = attached {
                let lease = access.lease();
                let (connection, incoming) = Connection::new(client, tab, access);
                (
                    Backend::Attached(connection),
                    SharedScreen::Empty,
                    if lease.is_some() {
                        "Attached terminal"
                    } else {
                        "Read-only terminal"
                    },
                    Some(incoming),
                    lease,
                )
            } else if let Some((binding, tab, frontend_replies)) = peer {
                let mut terminal = Terminal::new(80, 24)?;
                terminal.set_colors(&theme.colors())?;
                (
                    Backend::Peer(Box::new(super::peer::Peer::new(
                        terminal,
                        binding,
                        tab,
                        frontend_replies,
                    )?)),
                    SharedScreen::Empty,
                    "Connecting…",
                    None,
                    None,
                )
            } else {
                let mut terminal = Terminal::new(80, 24)?;
                terminal.set_colors(&theme.colors())?;
                terminal.feed(FIXTURE)?;
                terminal.drain_replies()?;
                let screen = SharedScreen::Fixture(Rc::new(terminal.render()?));
                (
                    Backend::Fixture(terminal),
                    screen,
                    "Fixture | no PTY",
                    None,
                    None,
                )
            };
        Ok(cx.new(|cx: &mut Context<Self>| {
            let focus = cx.focus_handle();
            focus.focus(window);
            let presentation_focused = window.is_window_active() && focus.is_focused(window);
            cx.on_blur(&focus, window, |view, window, cx| {
                view.cancel_input(cx);
                view.sync_presentation_focus(window, cx);
            })
            .detach();
            cx.on_focus(&focus, window, |view, window, cx| {
                view.sync_presentation_focus(window, cx)
            })
            .detach();
            cx.observe_window_activation(window, |view, window, cx| {
                view.sync_presentation_focus(window, cx)
            })
            .detach();
            if let Backend::Peer(peer) = &backend {
                // Pump the peer state machine when its connection changes or work is queued.
                let (mut state, mut kick) = peer.changes();
                cx.spawn_in(window, async move |view, cx| {
                    loop {
                        if view.update(cx, |view, cx| view.poll_peer(cx)).is_err() {
                            return;
                        }
                        tokio::select! {
                            changed = state.changed() => if changed.is_err() { return },
                            changed = kick.changed() => if changed.is_err() { return },
                        }
                    }
                })
                .detach();
            }
            let mut view = Self {
                backend,
                focus,
                preedit: String::new(),
                clipboard_keys: 0,
                cell_width,
                font,
                line_height,
                font_size: renderer::FONT_SIZE,
                theme,
                bounds: None,
                selection_drag: super::selection::SelectionDrag::default(),
                link_press: None,
                links: super::links::LinkState::default(),
                wheel: Default::default(),
                scrollbar: super::scrollbar::Scrollbar::default(),
                screen,
                semantic_binding: None,
                semantic_refresh: None,
                cursor_reset_epoch: 0,
                status: status.into(),
                notice: None,
                failed: false,
                control_blocked: false,
                closing: false,
                close_pending: false,
                detached: false,
                presentation_focused,
                phase: None,
                grid: (80, 24),
                lease,
                captured_settlement: None,
                mouse_tracking: false,
                alternate_scroll: false,
                next_request: 0,
                pending: HashMap::new(),
                outbox: runtime::Outbox::default(),
                delivery: runtime::DeliveryState::default(),
            };
            if let Some(incoming) = incoming {
                runtime::native::start_worker(&view, incoming, cx);
                view.sync_configuration();
                view.start_frames(cx);
            }
            view
        }))
    }
}
