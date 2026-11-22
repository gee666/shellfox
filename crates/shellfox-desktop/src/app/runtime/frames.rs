use super::{native::Access, *};
use crate::app::SharedScreen;
use shellfox_service::native::{NativeAdmissionError, NativeFrame};
use std::sync::Arc;

const BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(16);
const ATTACHED_PACE: std::time::Duration = std::time::Duration::from_millis(16);

/// What a frame fetch did.
enum Fetched {
    Frame(anyhow::Result<Option<NativeFrame>>),
    /// Canceled, kicked (focus changed) or the view stopped presenting.
    Interrupted,
}

impl TerminalView {
    /// Fetches a frame whenever the host signals the tab may have changed or the
    /// view kicks itself (focus). An attached desktop paces its reads instead.
    pub(in crate::app) fn start_frames(&self, cx: &mut Context<Self>) {
        let Backend::Attached(connection) = &self.backend else {
            return;
        };
        let client = connection.client.clone();
        let access = connection.access.clone();
        let tab = connection.tab;
        let mut life = connection.frame_cancellation();
        cx.spawn(async move |view, cx| {
            let mut changes = client.watches().await.map(|watches| watches.frames.subscribe(tab));
            loop {
                life.borrow_and_update();
                let Ok(focused) = view.update(cx, |view, _| view.frame_gate()) else {
                    return;
                };
                let fetched = match focused {
                    Some(focused) => {
                        if let Some(changes) = &mut changes {
                            changes.borrow_and_update();
                        }
                        tokio::select! {
                            biased;
                            changed = life.changed() => {
                                if changed.is_err() {
                                    return;
                                }
                                Fetched::Interrupted
                            }
                            result = fetch(&client, tab, &access, focused) => Fetched::Frame(result),
                        }
                    }
                    None => Fetched::Interrupted,
                };
                let interrupted = matches!(fetched, Fetched::Interrupted);
                if let Fetched::Frame(result) = fetched {
                    let Ok(busy) = view.update(cx, |view, cx| view.accept_frame_result(result, cx)) else {
                        return;
                    };
                    if busy {
                        // Admission was full; retry shortly instead of waiting for a change.
                        cx.background_executor().timer(BUSY_RETRY).await;
                        continue;
                    }
                }
                // An interrupted fetch retries at once with the new state; an
                // idle gate waits for the next kick, cancel or resume.
                if interrupted && focused.is_some() {
                    continue;
                }
                if changes.is_none() && focused.is_some() {
                    // Attached hosts answer frame reads at once for now.
                    cx.background_executor().timer(ATTACHED_PACE).await;
                    continue;
                }
                let tab_changed = async {
                    match &mut changes {
                        Some(changes) if focused.is_some() => {
                            if changes.changed().await.is_err() {
                                std::future::pending::<()>().await;
                            }
                        }
                        _ => std::future::pending::<()>().await,
                    }
                };
                tokio::select! {
                    changed = life.changed() => if changed.is_err() { return },
                    () = tab_changed => {}
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
    fn accept_frame_result(&mut self, result: anyhow::Result<Option<NativeFrame>>, cx: &mut Context<Self>) -> bool {
        match result {
            Ok(Some(frame)) => self.accept_native_frame(frame, cx),
            Ok(None) => {}
            // Nothing crossed owner admission. A later frame fetch is safe;
            // unlike keyboard mutations it need not retain an old offer.
            Err(error)
                if matches!(
                    error.downcast_ref::<NativeAdmissionError>(),
                    Some(NativeAdmissionError::Full(_))
                ) => return true,
            Err(error)
                if error
                    .downcast_ref::<shellfox_runtime::RuntimeError>()
                    .is_some_and(|error| error.code == ErrorCode::ControlRequired) =>
            {
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
        let shown = (self.phase, self.mouse_tracking, self.status.clone(), self.notice.clone());
        self.delivery.consumed = self.delivery.consumed.max(frame.frame.sequence);
        self.delivery.delivered = frame.highest_delivered;
        self.delivery.exited = frame.exited;
        self.delivery.recovery_barrier = frame.recovery_barrier;
        self.phase = Some(frame.phase);
        self.mouse_tracking = frame.frame.mouse_tracking;
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
        if !same_frame || shown != (self.phase, self.mouse_tracking, self.status.clone(), self.notice.clone()) {
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
    pub(in crate::app) fn presentation_focus(&self, window: &gpui::Window) -> bool {
        window.is_window_active() && self.focus.is_focused(window) && !self.link_menu_open()
    }
    /// Focus is part of the frame request (terminal focus reporting), so a
    /// change refetches at once.
    pub(in crate::app) fn sync_presentation_focus(&mut self, window: &gpui::Window, cx: &mut Context<Self>) {
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
