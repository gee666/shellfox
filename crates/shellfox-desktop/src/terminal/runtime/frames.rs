use super::{native::Access, *};
use crate::terminal::SharedScreen;
use shellfox_service::native::{NativeAdmissionError, NativeFrame};
use std::sync::Arc;

const BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(16);

impl TerminalView {
    /// Keeps one frame read outstanding. The host holds it until the tab may
    /// have changed; a kick (focus) or cancel replaces it at once.
    pub(in crate::terminal) fn start_frames(&self, cx: &mut Context<Self>) {
        let Backend::Attached(connection) = &self.backend else {
            return;
        };
        let client = connection.client.clone();
        let access = connection.access.clone();
        let tab = connection.tab;
        let mut life = connection.frame_cancellation();
        cx.spawn(async move |view, cx| {
            loop {
                life.borrow_and_update();
                let Ok(focused) = view.update(cx, |view, _| view.frame_gate()) else {
                    return;
                };
                let Some(focused) = focused else {
                    // Stopped presenting; wait for a resume or kick.
                    if life.changed().await.is_err() {
                        return;
                    }
                    continue;
                };
                let result = tokio::select! {
                    biased;
                    changed = life.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        continue;
                    }
                    result = fetch(&client, tab, &access, focused) => result,
                };
                let Ok(busy) = view.update(cx, |view, cx| view.accept_frame_result(result, cx))
                else {
                    return;
                };
                if busy {
                    // Admission was full; retry shortly instead of waiting for a change.
                    cx.background_executor().timer(BUSY_RETRY).await;
                }
            }
        })
        .detach();
    }
    /// Some(focused) while this view may present frames.
    fn frame_gate(&mut self) -> Option<bool> {
        self.flush_outbox();
        (!self.failed && !self.closing && !self.control_blocked && !self.detached)
            .then_some(self.presentation_focused)
    }
    /// Returns true when admission was full and the fetch should be retried.
    fn accept_frame_result(
        &mut self,
        result: anyhow::Result<Option<NativeFrame>>,
        cx: &mut Context<Self>,
    ) -> bool {
        match result {
            Ok(Some(frame)) => self.accept_native_frame(frame, cx),
            Ok(None) => {}
            // Nothing crossed owner admission. A later frame fetch is safe;
            // unlike keyboard mutations it need not retain an old offer.
            Err(error)
                if matches!(
                    error.downcast_ref::<NativeAdmissionError>(),
                    Some(NativeAdmissionError::Full(_))
                ) =>
            {
                return true;
            }
            Err(error) if super::control_required(&error) => {
                self.block_control();
                cx.notify();
            }
            Err(error) => {
                self.fail(format!("Terminal presentation stopped: {error}"));
                cx.notify();
            }
        }
        false
    }
    fn accept_native_frame(&mut self, frame: NativeFrame, cx: &mut Context<Self>) {
        if self.detached {
            return;
        }
        let same_frame = matches!(&self.screen, SharedScreen::Native(previous) if Arc::ptr_eq(previous, &frame.presentation));
        let shown = (
            self.phase,
            self.mouse_tracking,
            self.status.clone(),
            self.notice.clone(),
        );
        self.delivery.consumed = self.delivery.consumed.max(frame.frame.sequence);
        self.delivery.delivered = frame.highest_delivered;
        self.delivery.exited = frame.exited;
        self.delivery.recovery_barrier = frame.recovery_barrier;
        self.phase = Some(frame.phase);
        self.mouse_tracking = frame.frame.mouse_tracking;
        self.alternate_scroll = frame.frame.alternate_scroll;
        // HostClient already accepted into bounded native state before ACK.
        // Always accept, including unchanged IDs, as delivery can catch up.
        if !same_frame {
            self.screen = SharedScreen::Native(frame.presentation);
        }
        if frame.phase == shellfox_runtime::Phase::Closed {
            self.outbox.stop_input(true);
        }
        if let Some(error) = frame.error {
            self.reject(error);
        } else if self.delivery.finished() {
            self.status = "PTY Closed".into();
        } else {
            self.status = format!("PTY {:?}", frame.phase);
        }
        if frame.replay_lost {
            self.recover();
        }
        // Repaint only when something visible changed.
        if !same_frame
            || shown
                != (
                    self.phase,
                    self.mouse_tracking,
                    self.status.clone(),
                    self.notice.clone(),
                )
        {
            cx.notify();
        }
    }
}

async fn fetch(
    client: &crate::host_client::HostClient,
    tab: shellfox_core::TabId,
    access: &Access,
    focused: bool,
) -> anyhow::Result<Option<NativeFrame>> {
    match access {
        Access::Control(attachment) => client.frame_focused(tab, attachment.clone(), focused).await,
        Access::Observer(attachment) => {
            client
                .observer_frame_focused(tab, attachment.clone(), focused)
                .await
        }
    }
}

impl TerminalView {
    pub(in crate::terminal) fn presentation_focus(&self, window: &gpui::Window) -> bool {
        window.is_window_active() && self.focus.is_focused(window) && !self.link_menu_open()
    }
    /// Focus is part of the frame request (terminal focus reporting), so a
    /// change refetches at once.
    pub(in crate::terminal) fn sync_presentation_focus(
        &mut self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let focused = self.presentation_focus(window);
        if focused == self.presentation_focused {
            return;
        }
        self.presentation_focused = focused;
        if !focused {
            self.cancel_input(cx);
        }
        self.report_peer_focus(focused);
        if let Backend::Attached(connection) = &self.backend {
            connection.kick_frames();
        }
    }
}
