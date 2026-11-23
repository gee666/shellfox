use crate::{error, Actor};
use shellfox_protocol::{AppError, CancellationToken, ErrorCode, RemoteCommand};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub enum OwnerCommand {
    Pair {
        actor: Actor,
    },
    Execute {
        actor: Actor,
        command: RemoteCommand,
    },
    Poll {
        actor: Actor,
    },
    Disconnect {
        actor: Actor,
    },
}
impl OwnerCommand {
    pub fn actor(&self) -> &Actor {
        match self {
            Self::Pair { actor }
            | Self::Execute { actor, .. }
            | Self::Poll { actor }
            | Self::Disconnect { actor } => actor,
        }
    }
    pub fn operation(&self) -> Option<&RemoteCommand> {
        if let Self::Execute { command, .. } = self {
            Some(command)
        } else {
            None
        }
    }
}
pub type OwnerResult = Result<serde_json::Value, AppError>;
pub type OwnerFuture<'a> = Pin<Box<dyn Future<Output = OwnerResult> + Send + 'a>>;
/// HTTP-side async boundary, not implemented on the non-Sync service itself.
pub trait RemoteHost: Send + Sync {
    fn request(&self, command: OwnerCommand) -> OwnerFuture<'_>;
}
/// Called only on the existing service owner thread. No Send or Sync requirement.
pub trait ServiceHost {
    fn dispatch(&mut self, command: OwnerCommand) -> OwnerResult;
}

type Wake = Arc<dyn Fn() + Send + Sync>;
#[derive(Clone)]
pub struct OwnerClient {
    sender: mpsc::Sender<OwnerRequest>,
    wake: Option<Wake>,
}
pub struct OwnerReceiver {
    receiver: mpsc::Receiver<OwnerRequest>,
}
pub struct OwnerRequest {
    pub command: OwnerCommand,
    reply: oneshot::Sender<OwnerResult>,
    cancel: CancellationToken,
}
pub fn owner_channel(capacity: usize) -> (OwnerClient, OwnerReceiver) {
    channel(capacity, None)
}
/// Like [`owner_channel`], but calls `on_enqueue` after each request is queued
/// so the owner thread can dispatch it at once. It is never called for a
/// refused request, runs on a network task and must not block.
pub fn owner_channel_with_wake(
    capacity: usize,
    on_enqueue: Arc<dyn Fn() + Send + Sync>,
) -> (OwnerClient, OwnerReceiver) {
    channel(capacity, Some(on_enqueue))
}
fn channel(capacity: usize, wake: Option<Wake>) -> (OwnerClient, OwnerReceiver) {
    let (sender, receiver) = mpsc::channel(capacity.clamp(1, 64));
    (OwnerClient { sender, wake }, OwnerReceiver { receiver })
}
impl RemoteHost for OwnerClient {
    fn request(&self, command: OwnerCommand) -> OwnerFuture<'_> {
        Box::pin(async move {
            let cancel = CancellationToken::new();
            // This exact request future owns recipient cancellation, including
            // timeout and post-send future drop. No detached M2 lifetime.
            let _cancel_on_drop = cancel.clone().drop_guard();
            let (reply, receiver) = oneshot::channel();
            // Never wait in an unbounded admission queue, never retry a mutation.
            self.sender
                .try_send(OwnerRequest {
                    command,
                    reply,
                    cancel,
                })
                .map_err(|_| {
                    error(
                        ErrorCode::NativeUnavailable,
                        "Service owner unavailable or busy. Operation was not queued.",
                        true,
                    )
                })?;
            if let Some(wake) = &self.wake {
                wake();
            }
            tokio::time::timeout(Duration::from_secs(25), receiver).await
                .map_err(|_| error(ErrorCode::NativeUnavailable, "Owner reply timed out. Outcome may be uncertain; do not retry a mutation automatically.", false))?
                .map_err(|_| error(ErrorCode::NativeUnavailable, "Owner reply lost. Outcome may be uncertain.", false))?
        })
    }
}
impl OwnerReceiver {
    pub async fn recv(&mut self) -> Option<OwnerRequest> {
        self.receiver.recv().await
    }
    pub fn try_recv(&mut self) -> Result<OwnerRequest, mpsc::error::TryRecvError> {
        self.receiver.try_recv()
    }
}
impl OwnerRequest {
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled() || self.reply.is_closed()
    }
    pub fn complete(self, result: OwnerResult) {
        if self.reply.send(result).is_err() {
            self.cancel.cancel();
        }
    }
    /// Prefer this to manual completion. Checks the epoch on the owner immediately
    /// before dispatch, holding authorization through the synchronous commit.
    pub fn dispatch(self, host: &mut impl ServiceHost) {
        if self.is_cancelled() {
            return;
        }
        // Cleanup must still run after revocation. Its unforgeable actor identifies
        // only that peer/view; the service must not perform host mutations here.
        let result = if matches!(self.command, OwnerCommand::Disconnect { .. }) {
            host.dispatch(self.command.clone())
        } else {
            let actor = self.command.actor().clone();
            let command = self.command.clone();
            actor.with_authorized(command.operation(), || {
                if self.is_cancelled() {
                    return Err(error(
                        ErrorCode::NativeUnavailable,
                        "Queued operation cancelled before dispatch.",
                        false,
                    ));
                }
                host.dispatch(self.command.clone())
            })
        };
        self.complete(result);
    }
}
