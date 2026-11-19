use crate::native::NativeResponse;
use crate::power::{worker::PowerWorker, PowerReport, PowerRequest, PowerStatus};
use crate::Service;
use anyhow::Result;
use tokio::sync::oneshot;

#[derive(Default)]
pub(crate) struct ServicePower {
    worker: PowerWorker,
    reply: Option<PendingPower>,
}
impl ServicePower {
    pub(crate) fn for_data_dir(directory: &std::path::Path) -> Self {
        Self {
            worker: PowerWorker::new(crate::power::SystemPower::for_data_dir(directory)),
            reply: None,
        }
    }
}

struct PendingPower {
    reply: PowerReply,
    cancel: shellfox_protocol::CancellationToken,
    new_enable: bool,
}
pub(crate) enum PowerReply {
    Native(oneshot::Sender<Result<NativeResponse>>),
    Local(oneshot::Sender<Result<serde_json::Value>>),
}
impl PowerReply {
    fn send(self, result: Result<PowerReport>) {
        match self {
            Self::Native(reply) => {
                let _ = reply.send(result.map(NativeResponse::Power));
            }
            Self::Local(reply) => {
                let _ = reply.send(result.and_then(|report| Ok(serde_json::to_value(report)?)));
            }
        }
    }
}
impl Service {
    pub(crate) fn request_power(
        &mut self,
        request: PowerRequest,
        cancel: shellfox_protocol::CancellationToken,
        reply: PowerReply,
    ) {
        self.pump_power();
        let allowed = match request {
            PowerRequest::Set { active: true } => self.assert_accepting(),
            _ => self.assert_owned(),
        };
        if let Err(error) = allowed {
            reply.send(Err(error));
            return;
        }
        match request {
            PowerRequest::Status {} => {
                reply.send(self.power.worker.admit_owner_receipt(&self.ownership_lost))
            }
            PowerRequest::Set { active } => {
                match self
                    .power
                    .worker
                    .start(active, cancel.clone(), self.ownership_lost.clone())
                {
                    Ok(()) => {
                        self.power.reply = Some(PendingPower {
                            reply,
                            cancel,
                            new_enable: active
                                && self.power.worker.report().status == PowerStatus::Inactive,
                        })
                    }
                    Err(error) => reply.send(Err(error)),
                }
            }
        }
    }
    pub(crate) fn pump_power(&mut self) {
        self.power.worker.poll();
        let report = self.power.worker.report();
        if report.pending {
            if self.power.worker.uncertain() {
                if let Some(pending) = self.power.reply.take() {
                    pending
                        .reply
                        .send(self.power.worker.admit_owner_receipt(&self.ownership_lost));
                }
            }
            return;
        }
        // Cancellation may race the worker's last check. Undo only the lease
        // created for that request, never an already-active Host lease.
        if self
            .power
            .reply
            .as_ref()
            .is_some_and(|pending| pending.new_enable && pending.cancel.is_cancelled())
            && matches!(report.status, PowerStatus::Active | PowerStatus::Partial)
        {
            let result = self.power.worker.start(
                false,
                shellfox_protocol::CancellationToken::new(),
                self.ownership_lost.clone(),
            );
            if let Err(error) = result {
                if let Some(pending) = self.power.reply.take() {
                    pending.reply.send(Err(error));
                }
            }
            return;
        }
        // A finished enable may race the shutdown fence after its last worker
        // check. Never deliver that stale Active receipt. Cleanup owns it now.
        if self.stopping && matches!(report.status, PowerStatus::Active | PowerStatus::Partial) {
            if let Some(pending) = self.power.reply.take() {
                pending.reply.send(Err(anyhow::anyhow!(
                    "Host stopped before keep-awake enable was confirmed."
                )));
            }
            return;
        }
        if let Some(pending) = self.power.reply.take() {
            pending
                .reply
                .send(self.power.worker.admit_owner_receipt(&self.ownership_lost));
        }
    }
    pub(crate) fn begin_stop_power(&mut self) {
        self.power.worker.begin_stop();
        self.pump_power();
        // Also start release before other shutdown resources can fail. Never
        // wait here for the platform or drop an unconfirmed worker/lease.
        let _ = self.power.worker.stop();
        self.pump_power();
    }
    pub(crate) fn stop_power(&mut self) -> Result<()> {
        let result = self.power.worker.stop();
        self.pump_power();
        result
    }
}
