use crate::native::NativeResponse;
use anyhow::Result;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::oneshot;

/// Legacy accepted wait keeps its canonical recipient token even when the
/// caller drops the receiver after owner admission.
pub struct ExperimentAccepted {
    receiver: oneshot::Receiver<Result<super::ExperimentResponse>>,
    _recipient: crate::grant::requests::RecipientCancellation,
}
impl ExperimentAccepted {
    pub(super) fn new(
        receiver: oneshot::Receiver<Result<super::ExperimentResponse>>,
        recipient: crate::grant::requests::RecipientCancellation,
    ) -> Self {
        Self {
            receiver,
            _recipient: recipient,
        }
    }
}
impl Future for ExperimentAccepted {
    type Output = std::result::Result<Result<super::ExperimentResponse>, oneshot::error::RecvError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().receiver).poll(cx)
    }
}
/// A request was admitted exactly once. Dropping this wait never retries it.
/// Remote helper control stays on its async manager, not the service owner.
pub enum NativeAccepted {
    Owner(NativeOwnerReply),
    Remote(
        oneshot::Receiver<
            std::result::Result<crate::remote::native::RemoteReceipt, shellfox_protocol::AppError>,
        >,
    ),
}
pub struct NativeOwnerReply {
    receiver: oneshot::Receiver<Result<NativeResponse>>,
    cancel: shellfox_protocol::CancellationToken,
    completed: bool,
}
impl Drop for NativeOwnerReply {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel.cancel();
        }
    }
}
impl NativeAccepted {
    pub(super) fn owner(
        receiver: oneshot::Receiver<Result<NativeResponse>>,
        cancel: shellfox_protocol::CancellationToken,
    ) -> Self {
        Self::Owner(NativeOwnerReply {
            receiver,
            cancel,
            completed: false,
        })
    }
}
impl std::fmt::Debug for NativeAccepted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeAccepted")
    }
}
impl Future for NativeAccepted {
    type Output = Result<NativeResponse>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut() {
            Self::Owner(reply) => match Pin::new(&mut reply.receiver).poll(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(result)) => {reply.completed=true;Poll::Ready(result)},
                Poll::Ready(Err(_)) => {reply.cancel.cancel();reply.completed=true;Poll::Ready(Err(anyhow::anyhow!("Service owner reply lost after acceptance; outcome is uncertain and action was not retried.")))},
            },
            Self::Remote(receiver) => match Pin::new(receiver).poll(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(Ok(receipt))) => Poll::Ready(Ok(match receipt.connect {
                    Some(result) => NativeResponse::RemoteConnect { state: receipt.state, result },
                    None => NativeResponse::Remote(receipt.state),
                })),
                Poll::Ready(Ok(Err(error))) => Poll::Ready(Err(anyhow::anyhow!("Remote control request failed: {}", error.message))),
                Poll::Ready(Err(_)) => Poll::Ready(Err(anyhow::anyhow!("Remote control reply lost after acceptance; outcome is uncertain and action was not retried."))),
            },
        }
    }
}
