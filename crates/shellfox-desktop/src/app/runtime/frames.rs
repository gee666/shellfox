use super::{native::Access, *};
use crate::app::SharedScreen;
use shellfox_service::native::{NativeAdmissionError, NativeFrame};
use std::sync::Arc;

impl TerminalView {
    pub(in crate::app) fn poll(&mut self, cx: &mut Context<Self>) {
        self.flush_outbox();
        if self.native_poll || self.failed || self.closing || self.control_blocked || self.detached
        {
            return;
        }
        let Backend::Attached(connection) = &self.backend else {
            return;
        };
        let client = connection.client.clone();
        let access = connection.access.clone();
        let tab = connection.tab;
        let focused = self.presentation_focused;
        let mut canceled = connection.frame_cancellation();
        self.native_poll = true;
        cx.spawn(async move |view, cx| {
            let frame = async {
                match access {
                    Access::Control(attachment) => {
                        client.frame_focused(tab, attachment, focused).await
                    }
                    Access::Observer(attachment) => {
                        client
                            .observer_frame_focused(tab, attachment, focused)
                            .await
                    }
                }
            };
            let already_canceled = *canceled.borrow();
            let result = if already_canceled {
                Ok(None)
            } else {
                tokio::select! {
                    biased;
                    _ = canceled.changed() => Ok(None),
                    result = frame => result,
                }
            };
            let _ = view.update(cx, |view, cx| {
                view.native_poll = false;
                match result {
                    Ok(Some(frame)) => view.accept_native_frame(frame, cx),
                    Ok(None) => {}
                    // Nothing crossed owner admission. A later frame poll is safe;
                    // unlike keyboard mutations it need not retain an old offer.
                    Err(error)
                        if matches!(
                            error.downcast_ref::<NativeAdmissionError>(),
                            Some(NativeAdmissionError::Full(_))
                        ) => {}
                    Err(error)
                        if error
                            .downcast_ref::<shellfox_runtime::RuntimeError>()
                            .is_some_and(|error| error.code == ErrorCode::ControlRequired) =>
                    {
                        view.block_control()
                    }
                    Err(error) => view.fail(format!("Terminal presentation stopped: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn accept_native_frame(&mut self, frame: NativeFrame, cx: &mut Context<Self>) {
        if self.detached {
            return;
        }
        let same_frame = matches!(&self.screen, SharedScreen::Native(previous) if Arc::ptr_eq(previous, &frame.presentation));
        self.delivery.consumed = self.delivery.consumed.max(frame.frame.sequence);
        self.delivery.delivered = frame.highest_delivered;
        self.delivery.exited = frame.exited;
        self.delivery.recovery_barrier = frame.recovery_barrier;
        self.phase = Some(frame.phase);
        self.mouse_tracking = frame.frame.mouse_tracking;
        // HostClient already accepted into bounded native state before ACK.
        // Always poll/accept, including unchanged IDs, as delivery can catch up.
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
        if !same_frame {
            cx.notify();
        }
    }
}
