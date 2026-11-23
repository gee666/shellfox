//! Narrow requests to the existing actor and one retained remote dispatcher handoff.
use super::*;
use shellfox_core::{ApprovalStoreRecord, ApprovalStoreSnapshot};
use shellfox_protocol::{AppError, ErrorCode};
use shellfox_remote::{ApprovalRepository, ApprovalRepositoryFuture};
use std::sync::{Arc, Mutex};

pub(super) enum ApprovalOperation {
    Load,
    Commit {
        expected: u64,
        records: Vec<ApprovalStoreRecord>,
    },
}
pub(super) struct ApprovalTask {
    operation: ApprovalOperation,
    reply: oneshot::Sender<std::result::Result<ApprovalStoreSnapshot, AppError>>,
}
fn unavailable(message: &str) -> AppError {
    AppError {
        code: ErrorCode::StorageFailed,
        message: message.into(),
        retryable: false,
    }
}
struct ActorRepository {
    sender: SyncSender<Task>,
    lost: CancellationToken,
}
impl ActorRepository {
    async fn request(
        &self,
        operation: ApprovalOperation,
    ) -> std::result::Result<ApprovalStoreSnapshot, AppError> {
        if self.lost.is_cancelled() {
            return Err(unavailable("Original approval owner is lost."));
        }
        let (reply, received) = oneshot::channel();
        self.sender
            .try_send(Task::Approvals(Box::new(ApprovalTask { operation, reply })))
            .map_err(|_| unavailable("Approval request was not queued; no retry was made."))?;
        // No caller cancellation token is sent. An accepted commit keeps running.
        let result = received.await.map_err(|_| {
            unavailable("Approval reply is unknown; commit may have occurred. No retry was made.")
        })?;
        if self.lost.is_cancelled() {
            return Err(unavailable(
                "Ownership changed; approval commit outcome is unavailable.",
            ));
        }
        result
    }
}
impl ApprovalRepository for ActorRepository {
    fn load(&self) -> ApprovalRepositoryFuture<'_, ApprovalStoreSnapshot> {
        Box::pin(self.request(ApprovalOperation::Load))
    }
    fn commit(
        &self,
        expected: u64,
        records: Vec<ApprovalStoreRecord>,
    ) -> ApprovalRepositoryFuture<'_, ApprovalStoreSnapshot> {
        Box::pin(async move {
            let validated = ApprovalStoreSnapshot {
                revision: expected,
                approvals: records,
            };
            validated
                .validate()
                .map_err(|_| unavailable("Invalid approval request bound or data."))?;
            self.request(ApprovalOperation::Commit {
                expected,
                records: validated.approvals,
            })
            .await
        })
    }
}
impl OwnerClient {
    /// Trusted local persistence only; this does not install or revoke runtime grants.
    /// The supplied token only adds denial. The actor independently checks its
    /// ORIGINAL native ownership before and after each request.
    pub fn approval_repository(&self, lost: CancellationToken) -> Arc<dyn ApprovalRepository> {
        Arc::new(ActorRepository {
            sender: self.sender.clone(),
            lost,
        })
    }
}
pub(super) fn handle(service: &mut Service, task: ApprovalTask, lost: &CancellationToken) {
    let result = if lost.is_cancelled() || service.stopping {
        Err(unavailable("Approval owner is closing."))
    } else {
        let result = match task.operation {
            ApprovalOperation::Load => service.host().approval_snapshot(),
            ApprovalOperation::Commit { expected, records } => {
                service.host().commit_approvals(expected, records)
            }
        };
        result.map_err(|error| unavailable(&error.message))
    };
    let result = if lost.is_cancelled() {
        Err(unavailable(
            "Ownership changed; approval outcome is unknown.",
        ))
    } else {
        result
    };
    let _ = task.reply.send(result);
}

struct InstallState {
    dispatcher: Option<Box<crate::remote::RemoteDispatcher>>,
    queued: bool,
    failed: bool,
}
/// Original envelope and actor retain this SAME slot and receipt. The dispatcher
/// never becomes an async install future's by-value local or a recoverable task payload.
pub(crate) struct RemoteInstall {
    state: Mutex<InstallState>,
    receipt: tokio::sync::watch::Sender<Option<std::result::Result<(), &'static str>>>,
}
impl RemoteInstall {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(InstallState {
                dispatcher: None,
                queued: false,
                failed: false,
            }),
            receipt: tokio::sync::watch::channel(None).0,
        })
    }
    pub(crate) fn prepare(&self, dispatcher: Box<crate::remote::RemoteDispatcher>) -> Result<()> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poison) => {
                let mut state = poison.into_inner();
                if state.dispatcher.is_none() {
                    state.dispatcher = Some(dispatcher);
                }
                state.failed = true;
                self.receipt.send_replace(Some(Err(
                    "Remote installer poisoned; original state retained",
                )));
                anyhow::bail!("Remote installer poisoned; original state retained");
            }
        };
        anyhow::ensure!(
            !state.failed && !state.queued && state.dispatcher.is_none(),
            "Remote dispatcher is already retained"
        );
        state.dispatcher = Some(dispatcher);
        Ok(())
    }
    fn enqueue(self: &Arc<Self>, owner: &ServiceOwner) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Remote installer unavailable"))?;
        anyhow::ensure!(
            !state.failed && !state.queued && state.dispatcher.is_some(),
            "Remote installer cannot be replayed"
        );
        match owner.sender.try_send(Task::InstallRemote(self.clone())) {
            Ok(()) => {
                state.queued = true;
                Ok(())
            }
            Err(_) => {
                state.failed = true;
                self.receipt.send_replace(Some(Err(
                    "Remote installer was not queued; original state retained",
                )));
                Err(anyhow::anyhow!(
                    "Remote installer was not queued; original dispatcher retained"
                ))
            }
        }
    }
    pub(super) fn acknowledge(&self, accepted: bool) {
        let ready = self.state.lock().is_ok_and(|mut state| {
            let ready = accepted && !state.failed && state.queued && state.dispatcher.is_some();
            if !ready {
                state.failed = true;
            }
            ready
        });
        self.receipt.send_replace(Some(if ready {
            Ok(())
        } else {
            Err("Remote dispatcher installation unavailable; original state retained")
        }));
    }
    pub(super) fn bind_wake(&self, wake: super::OwnerWake) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(dispatcher) = state.dispatcher.as_mut() {
                crate::OwnerDispatcher::bind_wake(dispatcher.as_mut(), wake);
            }
        }
    }
    pub(super) fn dispatch_one(&self, service: &mut Service) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Remote installer unavailable"))?;
        anyhow::ensure!(!state.failed, "Remote installation is unavailable");
        state
            .dispatcher
            .as_mut()
            .context("Remote dispatcher missing")?
            .dispatch_one(service);
        Ok(())
    }
    pub(crate) async fn install(
        self: &Arc<Self>,
        owner: &ServiceOwner,
        closing: &shellfox_protocol::CancellationToken,
    ) -> Result<()> {
        // A cancelled wait resumes this same retained receipt, never another install.
        let queued = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Remote installer unavailable"))?
            .queued;
        if !queued {
            anyhow::ensure!(
                !closing.is_cancelled(),
                "Remote owner is closing; original dispatcher retained"
            );
            self.enqueue(owner)?;
        }
        let mut receipt = self.receipt.subscribe();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(result) = *receipt.borrow() {
                return result.map_err(anyhow::Error::msg);
            }
            if closing.is_cancelled()
                || owner.abandon.is_cancelled()
                || owner
                    .thread
                    .as_ref()
                    .is_some_and(|thread| thread.is_finished())
                || std::time::Instant::now() >= deadline
            {
                let message = "Remote install reply is unknown; original state retained, no retry";
                if let Ok(mut state) = self.state.lock() {
                    state.failed = true;
                }
                self.receipt.send_replace(Some(Err(message)));
                return Err(anyhow::anyhow!(message));
            }
            tokio::select! {
                result = receipt.changed() => result.context("Remote installer receipt unavailable; original state retained")?,
                _ = tokio::time::sleep(Duration::from_millis(20)) => {},
            }
        }
    }
}
#[cfg(test)]
mod tests;
