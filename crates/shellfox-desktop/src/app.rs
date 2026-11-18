mod links;
mod open;
mod pointer;
mod runtime;
mod screen;
mod selection;
mod semantic_selection;
use crate::renderer;
pub use crate::renderer::TerminalTheme;
use anyhow::{Context as _, Result};
use gpui::{Context, FocusHandle, Render, Window, div, prelude::*, px, rgb};
use screen::SharedScreen;
use shellfox_ghostty_vt::{Terminal, ViewportPoint};
use shellfox_runtime::{Command, Lease, Phase};
use std::{collections::HashMap, rc::Rc};

pub const FIXTURE: &[u8] = b"\x1b[2J\x1b[HShellfox GPUI / libghostty-vt\r\n\x1b[32mFixture mode\x1b[0m. No shell or production data.\r\n\x1b[34mBlue\x1b[0m \x1b[1mBold\x1b[0m \x1b[4mUnderline\x1b[0m\r\n\r\nUse --shell on a verified PTY platform.\r\n";

// A live view holds only a client of the app's central service, never a runtime.
enum Backend {
    Fixture(Terminal),
    Attached(runtime::native::Connection),
}

pub struct TerminalView {
    backend: Backend,
    pub focus: FocusHandle,
    pub preedit: String,
    clipboard_keys: u8,
    pub cell_width: f32,
    pub font: gpui::Font,
    pub line_height: f32,
    pub(crate) font_size: f32,
    pub(crate) theme: TerminalTheme,
    bounds: Option<gpui::Bounds<gpui::Pixels>>,
    selection_drag: selection::SelectionDrag,
    link_press: Option<ViewportPoint>,
    wheel_remainder: gpui::Point<f32>,
    pub(crate) screen: SharedScreen,
    pub(crate) semantic_binding: Option<crate::terminal_semantics::TerminalSemantics>,
    pub(crate) cursor_reset_epoch: u64,
    status: String,
    notice: Option<String>,
    failed: bool,
    control_blocked: bool,
    closing: bool,
    close_pending: bool,
    detached: bool,
    native_poll: bool,
    presentation_focused: bool,
    phase: Option<Phase>,
    grid: (u16, u16),
    lease: Option<Lease>,
    captured_settlement: Option<shellfox_host::CapturedSettlement>,
    mouse_tracking: bool,
    next_request: u64,
    pending: HashMap<u64, runtime::Pending>,
    outbox: runtime::Outbox,
    delivery: runtime::DeliveryState,
}

impl TerminalView {
    fn reject(&mut self, error: impl std::fmt::Display) {
        let error = error.to_string();
        self.notice = Some(error.clone());
        self.status = error;
    }

    pub fn fail(&mut self, error: impl std::fmt::Display) {
        self.failed = true;
        if let Backend::Attached(connection) = &self.backend {
            connection.cancel_frames();
        }
        self.status = format!("Stopped: {error}");
        eprintln!("{}", self.status);
        // A view error does not own process cleanup. Finish accepted releases
        // in order, then detach this view only; the service retains the PTY.
        if let Err(error) = self.cancel_held_input(false) {
            self.reject(error);
        }
        if matches!(self.backend, Backend::Attached(_)) && !self.close_pending {
            self.close_pending = self.submit(Command::RetryClose, runtime::Pending::Close);
        }
    }
    pub fn presentation_frame(&self) -> Option<std::sync::Arc<shellfox_runtime::ViewFrame>> {
        match &self.screen {
            SharedScreen::Native(frame) => Some(frame.clone()),
            _ => None,
        }
    }
    pub fn set_captured_settlement(&mut self, marker: Option<String>, cx: &mut Context<Self>) {
        self.captured_settlement = Some(shellfox_host::CapturedSettlement::Captured(marker));
        cx.notify();
    }
    pub fn is_closed(&self) -> bool {
        matches!(self.backend, Backend::Fixture(_)) || self.detached
    }
    pub fn close(&mut self) -> bool {
        if self.is_closed() {
            return true;
        }
        if !self.closing {
            if let Err(error) = self.cancel_held_input(false) {
                self.reject(error);
            }
            self.flush_outbox();
        }
        self.closing = true;
        if let Backend::Attached(connection) = &self.backend {
            connection.cancel_frames();
        }
        if !self.close_pending {
            self.close_pending = self.submit(Command::RetryClose, runtime::Pending::Close);
            self.status = "Detaching terminal view".into();
        }
        false
    }
    fn apply_presentation(
        &mut self,
        action: shellfox_runtime::PresentationAction,
        fixture: impl FnOnce(&mut Terminal) -> shellfox_ghostty_vt::Result<()>,
    ) -> Result<()> {
        anyhow::ensure!(
            !self.failed && !self.closing,
            "terminal surface has stopped"
        );
        match &mut self.backend {
            Backend::Fixture(terminal) => {
                fixture(terminal)?;
                self.screen = SharedScreen::Fixture(Rc::new(terminal.render()?));
            }
            Backend::Attached(connection) => {
                let view = connection.access.presentation_lease();
                anyhow::ensure!(
                    self.submit(
                        Command::Presentation { view, action },
                        runtime::Pending::Applied
                    ),
                    "terminal presentation queue is full"
                );
            }
        }
        Ok(())
    }
    fn apply_terminal(
        &mut self,
        command: impl FnOnce(Lease) -> Command,
        fixture: impl FnOnce(&mut Terminal) -> shellfox_ghostty_vt::Result<()>,
    ) -> Result<()> {
        anyhow::ensure!(!self.failed && !self.closing, "terminal has stopped");
        match &mut self.backend {
            Backend::Fixture(terminal) => {
                fixture(terminal)?;
                self.screen = SharedScreen::Fixture(Rc::new(terminal.render()?));
            }
            Backend::Attached(_) => {
                let lease = self.lease.context("This terminal view is read-only")?;
                anyhow::ensure!(
                    self.submit(command(lease), runtime::Pending::Applied),
                    "terminal command queue is full"
                );
            }
        }
        Ok(())
    }
}

impl Drop for TerminalView {
    fn drop(&mut self) {
        if matches!(self.backend, Backend::Attached(_)) {
            self.close();
            let outbox = std::mem::take(&mut self.outbox);
            if let Backend::Attached(connection) = &mut self.backend {
                connection.retire(outbox, self.next_request);
            }
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut terminal = div()
            .id("terminal")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .relative()
            .min_w_0()
            .min_h_0()
            .bg(rgb(self.theme.background))
            .text_color(rgb(self.theme.foreground))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::physical_key_up))
            .on_mouse_move(cx.listener(Self::pointer_move))
            .on_scroll_wheel(cx.listener(Self::pointer_scroll));
        for button in gpui::MouseButton::all() {
            terminal = terminal
                .on_mouse_down(button, cx.listener(Self::pointer_down))
                .on_mouse_up(button, cx.listener(Self::pointer_up))
                .on_mouse_up_out(button, cx.listener(Self::pointer_up));
        }
        terminal
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .pl(px(8.))
                    .pr(px(2.))
                    .py(px(8.))
                    .child(renderer::surface(cx.entity(), self.focus.clone())),
            )
            .when(
                self.failed || self.notice.is_some() || !self.preedit.is_empty(),
                |terminal| {
                    let error = if self.failed {
                        Some(self.status.clone())
                    } else {
                        self.notice.clone()
                    };
                    let text = match (error, self.preedit.is_empty()) {
                        (Some(error), false) => format!("{error} | {}", self.preedit),
                        (Some(error), true) => error,
                        (None, _) => self.preedit.clone(),
                    };
                    terminal.child(
                        div()
                            .absolute()
                            .bottom(px(0.))
                            .left(px(8.))
                            .right(px(2.))
                            .min_h(px(renderer::NOTICE_HEIGHT))
                            .text_size(px(11.))
                            .bg(rgb(self.theme.background))
                            .child(text),
                    )
                },
            )
    }
}
