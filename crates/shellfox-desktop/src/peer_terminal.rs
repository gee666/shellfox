//! Native peer terminal: actual SDK owner/parser, never a local NativeFrame or
//! invented Runtime lease. Networking is the retained per-window peer client.
use crate::{
    host_client::peer::{Connection, Transport},
    renderer::{self, TerminalTheme},
    root::source::PeerCycle,
    theme::Palette,
};
use anyhow::{Result, anyhow};
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Font, Pixels, Render, Window, div, prelude::*, px,
};
use shellfox_ghostty_vt::Screen;
use shellfox_mobile::{ManagerOwnerView, ManagerRecovery};
use shellfox_protocol::manager_v2 as m;
use std::{rc::Rc, sync::Arc, time::Duration};
#[path = "peer_terminal_composition.rs"]
mod composition;
#[path = "peer_keys.rs"]
mod keys;
#[path = "peer_terminal_network.rs"]
mod network;
#[path = "../../native_peer_pending.rs"]
mod pending;
#[path = "peer_terminal_pointer.rs"]
mod pointer;
#[path = "peer_terminal_render.rs"]
mod render;
#[path = "peer_terminal_surface.rs"]
mod surface;
pub(crate) struct PeerTerminalView {
    pub source: PeerCycle,
    pub sdk: Option<Arc<ManagerRecovery<Transport>>>,
    owner: ManagerOwnerView,
    screen: Option<Rc<Screen>>,
    focus: FocusHandle,
    font: Font,
    cell_width: f32,
    font_size: f32,
    line_height: f32,
    theme: TerminalTheme,
    bounds: Option<Bounds<Pixels>>,
    grid: Option<(u16, u16)>,
    pub ready: bool,
    pub input_state: Option<m::InputState>,
    pub error: Option<String>,
    retired: bool,
    frozen: bool,
    input_failed: bool,
    ended: bool,
    visible: bool,
    selection: Option<(u16, u16)>,
    buttons: u8,
    wheel: (f32, f32),
    pub generation: u64,
    ack_busy: bool,
    input_busy: bool,
    resize_busy: bool,
    input: pending::PendingInputs,
    pending_size: Option<(u16, u16)>,
    pub hints: m::InputHints,
    preedit: String,
}
impl PeerTerminalView {
    pub fn create(
        connection: Arc<Connection>,
        source: PeerCycle,
        prefs: &shellfox_core::Preferences,
        window: &Window,
        cx: &mut App,
    ) -> Result<(Entity<Self>, ManagerRecovery<Transport>)> {
        let (font, cell_width) =
            renderer::choose_font(window, &prefs.font_family, prefs.font_size)?;
        let palette = Palette::new(&prefs.accent_color, &prefs.background_color);
        let sdk =
            ManagerRecovery::from_shared_native0(connection.client(), 80, 24, Default::default())
                .map_err(|error| anyhow!("Native peer owner: {error:?}"))?;
        let owner = sdk.owner_view();
        let view = cx.new(|cx| Self {
            source,
            sdk: None,
            owner,
            screen: None,
            focus: cx.focus_handle(),
            font,
            cell_width,
            font_size: prefs.font_size,
            line_height: (prefs.font_size * renderer::LINE_HEIGHT / renderer::FONT_SIZE).ceil(),
            theme: TerminalTheme::from_colors(
                palette.background_rgb,
                palette.foreground_rgb,
                palette.accent_rgb,
            )
            .with_ansi_palette(palette.ansi),
            bounds: None,
            grid: None,
            ready: false,
            input_state: None,
            error: None,
            selection: None,
            buttons: 0,
            wheel: (0., 0.),
            retired: false,
            frozen: false,
            input_failed: false,
            ended: false,
            visible: true,
            generation: 1,
            ack_busy: false,
            input_busy: false,
            resize_busy: false,
            input: pending::PendingInputs::default(),
            pending_size: None,
            hints: m::InputHints {
                user_initiated: true,
                activity_hint: true,
                settled_at: None,
            },
            preedit: String::new(),
        });
        view.update(cx, |_, cx| Self::start_owner(cx));
        Ok((view, sdk))
    }
    pub fn apply_preferences(
        &mut self,
        prefs: &shellfox_core::Preferences,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let (font, width) = renderer::choose_font(window, &prefs.font_family, prefs.font_size)?;
        let palette = Palette::new(&prefs.accent_color, &prefs.background_color);
        self.font = font;
        self.cell_width = width;
        self.font_size = prefs.font_size;
        self.line_height = (prefs.font_size * renderer::LINE_HEIGHT / renderer::FONT_SIZE).ceil();
        self.theme = TerminalTheme::from_colors(
            palette.background_rgb,
            palette.foreground_rgb,
            palette.accent_rgb,
        )
        .with_ansi_palette(palette.ansi);
        if let Some(bounds) = self.bounds {
            self.set_bounds(bounds);
        }
        cx.notify();
        Ok(())
    }
    pub fn focus(&self, window: &mut Window) {
        self.focus.focus(window);
    }
    pub fn visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.preedit.clear();
        }
    }
    pub fn publish(
        &mut self,
        sdk: ManagerRecovery<Transport>,
        result: std::result::Result<m::Reply<m::Attachment>, shellfox_mobile::ManagerRecoveryError>,
        cx: &mut Context<Self>,
    ) {
        if self.retired {
            return;
        }
        match result {
            Ok(m::Reply::Success(m::Attachment::Attached(attached))) => {
                self.input_state = Some(attached.input_state);
                self.sdk = Some(Arc::new(sdk));
                self.ready = false;
                self.input_failed = false;
                self.error = None;
            }
            Ok(m::Reply::Success(m::Attachment::Unavailable { reason, .. })) => {
                self.error = Some(format!("Terminal unavailable: {reason:?}"))
            }
            Ok(m::Reply::Failure(error)) => {
                self.error = Some(format!("Attach rejected: {:?}", error.code))
            }
            Err(error) => self.error = Some(format!("Native attach failed: {error:?}")),
        }
        cx.notify();
    }
    pub fn pending_operations(&self) -> bool {
        self.ack_busy || self.input_busy || self.resize_busy || self.input.pending_count()!=0
    }
    pub fn freeze_inputs(&mut self, frozen: bool) -> bool {
        if frozen && self.pending_operations() {
            return false;
        }
        self.frozen = frozen;
        if frozen {
            self.preedit.clear();
        }
        true
    }
    fn cancel_unsent(&mut self) -> bool {
        self.input.cancel_unsent()
    }
    pub fn source_ended(&mut self) {
        self.ended = true;
        self.ready = false;
        self.input_state = None;
        self.error = Some("Source ended.".into());
    }
    pub fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.retired = true;
        self.ready = false;
        self.preedit.clear();
        self.cancel_unsent();
        self.sdk = None;
        let _ = self.owner.close();
    }
    pub fn set_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.bounds = Some(bounds);
        let grid = renderer::grid_size(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            self.cell_width,
            self.line_height,
        );
        if self.grid != Some(grid) {
            self.grid = Some(grid);
            self.pending_size = Some(grid);
        }
    }
    fn start_owner(cx: &mut Context<Self>) {
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let result = view.update(cx, |view, cx| {
                    if view.retired {
                        return false;
                    }
                    view.pump(cx);
                    true
                });
                if !matches!(result, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
    fn pump(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.owner.pump() {
            self.retire();
            self.error = Some(format!("Native owner: {error:?}"));
            cx.notify();
            return;
        }
        if self.sdk.is_none() {
            return;
        }
        if self.visible {
            match self.owner.frame() {
                Ok(frame) => {
                    let changed = self.screen.as_ref().is_none_or(|old| {
                        old.cols != frame.cols
                            || old.rows != frame.rows
                            || old.cells != frame.cells
                            || old.cursor != frame.cursor
                    });
                    if changed {
                        self.screen = Some(Rc::new(frame));
                        cx.notify();
                    }
                }
                Err(error) => {
                    self.retire();
                    self.error = Some(format!("Native frame: {error:?}"));
                    cx.notify();
                }
            }
        }
        self.ack(cx);
        self.send_next(cx);
        self.resize(cx);
    }
    fn admit(&mut self, input: m::Input, cx: &mut Context<Self>) {
        if self.retired || self.frozen || self.input_failed || self.ended || !self.ready {
            return;
        }
        if self.input.pending_count() >= 64 {
            self.error = Some("Peer input queue full; input was not sent.".into());
            cx.notify();
            return;
        }
        let Some(sdk) = &self.sdk else {
            return;
        };
        let mut hints = self.hints.clone();
        hints.activity_hint =
            !matches!(&input,m::Input::Key{event} if event.action==m::KeyActionV2::Release);
        match sdk.admit_input(input, hints) {
            Ok(input) => self.input.push(input),
            Err(error) => {
                self.error = Some(format!("Input not admitted: {error:?}"));
                cx.notify();
            }
        }
        self.send_next(cx);
    }
}
impl Drop for PeerTerminalView {
    fn drop(&mut self) {
        self.retire();
    }
}
