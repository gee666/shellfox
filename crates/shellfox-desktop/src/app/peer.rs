use super::{Backend, SharedScreen, TerminalView};
use crate::host_client::peer_v1::V1Binding;
use gpui::Context;
use shellfox_client::{ConnectionStatus, EventDisposition, ViewCursor};
use shellfox_ghostty_vt::{PresentationView, Terminal};
use shellfox_protocol::{
    AcknowledgeTerminal, AppError, DetachTerminal, ErrorCode, Generation, Reply, ScreenState,
    TabId, TerminalAttachment, TerminalEvent, WriteTerminal,
};
use std::{collections::VecDeque, rc::Rc, sync::Arc};

#[path = "peer_drive.rs"]
mod drive;
#[path = "peer_input.rs"]
mod input;
#[path = "peer_output.rs"]
mod output;
#[path = "peer_overlay.rs"]
mod overlay;
#[path = "peer_requests.rs"]
mod requests;
use requests::{Applied, Operation};

pub(super) struct Peer {
    pub terminal: Terminal,
    pub presentation: PresentationView,
    sequence: u64,
    binding: Arc<V1Binding>,
    tab: TabId,
    state: State,
    connection: u64,
    frozen: bool,
    visible: bool,
    frontend_replies: bool,
    pointer: input::Pointer,
    held_keys: Vec<shellfox_ghostty_vt::key::KeyEvent>,
    reported_focus: Option<bool>,
    in_flight: bool,
    detach_failed: bool,
    attachment_generation: Option<Generation>,
    desired_size: Option<(u16, u16)>,
    size_to_send: Option<(u16, u16)>,
    ack: Option<AcknowledgeTerminal>,
    writes: VecDeque<WriteTerminal>,
    /// Wakes the view's pump after local work was queued.
    kick: tokio::sync::watch::Sender<u64>,
}

enum State {
    Offline,
    Attaching(Vec<TerminalEvent>),
    Attached { cursor: ViewCursor, open: bool },
    Blocked(Generation),
    Unavailable(String),
}

impl Peer {
    pub fn new(
        mut terminal: Terminal,
        binding: Arc<V1Binding>,
        tab: TabId,
        frontend_replies: bool,
    ) -> shellfox_ghostty_vt::Result<Self> {
        let presentation = terminal.new_presentation_view()?;
        Ok(Self {
            terminal,
            presentation,
            sequence: 0,
            binding,
            tab,
            state: State::Offline,
            connection: 0,
            frozen: false,
            visible: true,
            frontend_replies,
            pointer: input::Pointer::default(),
            held_keys: Vec::new(),
            reported_focus: None,
            in_flight: false,
            detach_failed: false,
            attachment_generation: None,
            desired_size: None,
            size_to_send: None,
            ack: None,
            writes: VecDeque::new(),
            kick: tokio::sync::watch::channel(0).0,
        })
    }

    fn clear_operations(&mut self) {
        self.ack = None;
        self.size_to_send = None;
        self.writes.clear();
        self.pointer = input::Pointer::default();
        self.held_keys.clear();
        self.reported_focus = None;
    }

    pub(super) fn can_write(&self) -> bool {
        let state = self.binding.state();
        !self.frozen
            && self.visible
            && state.status == ConnectionStatus::Connected
            && state.connection_id == self.connection
            && matches!(self.state, State::Attached { open: true, .. })
    }

    pub(super) fn resize(&mut self, grid: (u16, u16)) {
        self.desired_size = Some(grid);
        self.size_to_send = Some(grid);
        self.kick();
    }

    fn queue(&mut self, bytes: Vec<u8>, user: bool) -> anyhow::Result<()> {
        if bytes.is_empty() || !self.can_write() {
            return Ok(());
        }
        let State::Attached { cursor, .. } = &self.state else {
            return Ok(());
        };
        anyhow::ensure!(
            self.writes.len() < 64,
            "Terminal input queue is full; input was not sent."
        );
        let write = cursor
            .write(String::from_utf8(bytes)?, Some(user), self.connection)
            .map_err(|_| {
                anyhow::anyhow!("Terminal input exceeds the v1 limit or control was lost.")
            })?;
        self.writes.push_back(write);
        self.kick();
        Ok(())
    }
    fn kick(&self) {
        self.kick.send_modify(|count| *count = count.wrapping_add(1));
    }
    /// Fires on connection state changes and on locally queued work.
    pub(super) fn changes(&self) -> (tokio::sync::watch::Receiver<u64>, tokio::sync::watch::Receiver<u64>) {
        (self.binding.state_changes(), self.kick.subscribe())
    }
}

impl TerminalView {
    fn peer_mut(&mut self) -> Option<&mut Peer> {
        match &mut self.backend {
            Backend::Peer(peer) => Some(peer),
            _ => None,
        }
    }

    pub(crate) fn freeze_peer(&mut self, frozen: bool) {
        if let Some(peer) = self.peer_mut() {
            peer.frozen = frozen;
            peer.kick();
            if frozen {
                self.preedit.clear();
            }
        }
    }

    pub(crate) fn peer_pending(&self) -> bool {
        matches!(&self.backend, Backend::Peer(peer) if peer.in_flight || !peer.writes.is_empty())
    }

    pub(crate) fn peer_attaching(&self) -> bool {
        matches!(&self.backend, Backend::Peer(peer) if matches!(peer.state, State::Attaching(_)))
    }

    pub(crate) fn peer_released(&self) -> bool {
        matches!(&self.backend, Backend::Peer(peer) if !peer.visible && !peer.in_flight && peer.attachment_generation.is_none())
    }

    pub(crate) fn set_peer_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if !self.peer_mut().is_some_and(|peer| peer.visible != visible) {
            return;
        }
        if !visible {
            self.report_peer_focus(false);
            self.cancel_peer_pointer();
            self.preedit.clear();
        }
        if let Some(peer) = self.peer_mut() {
            peer.visible = visible;
            peer.detach_failed = false;
        }
        self.poll_peer(cx);
        cx.notify();
    }

    pub(crate) fn peer_events(
        &mut self,
        events: Vec<TerminalEvent>,
        connection: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(peer) = self.peer_mut().filter(|peer| peer.connection == connection) else {
            return;
        };
        if let State::Attaching(staged) = &mut peer.state {
            if staged.len() + events.len() <= 1024 {
                staged.extend(events);
            } else {
                peer.unavailable("Terminal output exceeded the receive window.");
            }
        } else {
            let mut changed = false;
            for event in events {
                match peer.consume(event) {
                    Ok(render) => changed |= render,
                    Err(error) => {
                        peer.unavailable(error.to_string());
                        break;
                    }
                }
            }
            if changed {
                self.refresh_peer_screen();
            }
        }
        self.poll_peer(cx);
        cx.notify();
    }

    pub(super) fn refresh_peer_screen(&mut self) {
        let Some(peer) = self.peer_mut() else {
            return;
        };
        let result = (|| {
            let rendered = peer.terminal.view_render(&mut peer.presentation)?;
            let mut cursor = peer.terminal.cursor_visual()?;
            cursor.x = rendered.screen.cursor.x;
            cursor.y = rendered.screen.cursor.y;
            cursor.visible = rendered.screen.cursor.visible;
            let selection = peer.terminal.view_selection(&mut peer.presentation)?;
            let frame = super::screen::PeerFrame {
                rendered,
                cursor,
                selection,
                sequence: peer.sequence,
            };
            Ok::<_, shellfox_ghostty_vt::Error>((frame, peer.terminal.mouse_tracking()?))
        })();
        match result {
            Ok((frame, tracking)) => {
                self.screen = SharedScreen::Peer(Rc::new(frame));
                self.mouse_tracking = tracking;
            }
            Err(error) => peer.unavailable(error.to_string()),
        }
    }
}
