use super::*;
use shellfox_mobile::{ManagerRecoveryError, Status};
impl PeerTerminalView {
    pub(super) fn ack(&mut self, cx: &mut Context<Self>) {
        if self.ack_busy || self.retired || self.frozen {
            return;
        }
        let Some(sdk) = self.sdk.clone() else {
            return;
        };
        self.ack_busy = true;
        let generation = self.generation;
        cx.spawn(async move |view, cx| {
            let result = sdk.acknowledge_consumed().await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.ack_busy = false;
                if view.retired {
                    return;
                }
                let changed = match result {
                    Ok(m::Reply::Success(_)) => {
                        let ready = !view.ended && !view.input_failed;
                        let changed = view.ready != ready;
                        view.ready = ready;
                        changed
                    }
                    Err(ManagerRecoveryError::Engine(Status::Busy)) => false,
                    Ok(m::Reply::Failure(error)) => {
                        view.ready = false;
                        view.error = Some(format!("Native ACK rejected: {:?}", error.code));
                        true
                    }
                    Err(error) => {
                        view.ready = false;
                        view.error = Some(format!("Native ACK failed: {error:?}"));
                        true
                    }
                };
                if changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn send_next(&mut self, cx: &mut Context<Self>) {
        if self.input_busy || self.retired || self.frozen || self.input_failed {
            return;
        }
        let Some(sdk) = self.sdk.clone() else {
            return;
        };
        let Some(input) = self.input.pop() else {
            return;
        };
        self.input_busy = true;
        let generation = self.generation;
        cx.spawn(async move |view, cx| {
            let result = sdk.execute_input(input).await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation { return; }
                view.input_busy=false;
                if view.retired { return; }
                let failed=match result {
                    Ok(m::Reply::Success(_)) => false,
                    Ok(m::Reply::Failure(error)) => {
                        view.error = Some(format!("Input rejected: {:?}; tail canceled, reconnect before more input.", error.code));true
                    }
                    Err(error) => {
                        view.error = Some(format!("Input outcome unknown: {error:?}; not replayed. Reconnect before more input."));true
                    }
                };
                if failed{
                    view.input_failed=true;view.ready=false;
                    if !view.cancel_unsent(){view.retire();}
                }else{view.send_next(cx);}
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn resize(&mut self, cx: &mut Context<Self>) {
        if self.resize_busy || self.retired || self.frozen || !self.ready {
            return;
        }
        let Some(sdk) = self.sdk.clone() else {
            return;
        };
        let Some((cols, rows)) = self.pending_size.take() else {
            return;
        };
        let pending = match sdk.admit_resize(cols, rows) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(format!("Resize not admitted: {error:?}"));
                return;
            }
        };
        self.resize_busy = true;
        let generation = self.generation;
        cx.spawn(async move |view, cx| {
            let result = sdk.execute_resize(pending).await;
            let _ = view.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                view.resize_busy = false;
                if view.retired {
                    return;
                }
                match result {
                    Ok(m::Reply::Success(_)) => {}
                    Ok(m::Reply::Failure(error)) => {
                        view.error = Some(format!("Resize rejected: {:?}", error.code))
                    }
                    Err(error) => view.error = Some(format!("Resize failed: {error:?}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn take_for_control(&mut self) -> Result<ManagerRecovery<Transport>> {
        if self.frozen || self.pending_operations() {
            return Err(anyhow!("Wait for this view's pending operation."));
        }
        let sdk = self
            .sdk
            .take()
            .ok_or_else(|| anyhow!("Terminal is not attached."))?;
        match Arc::try_unwrap(sdk) {
            Ok(sdk) => {
                self.ready = false;
                Ok(sdk)
            }
            Err(sdk) => {
                self.sdk = Some(sdk);
                Err(anyhow!("Terminal operation is still active."))
            }
        }
    }
}
