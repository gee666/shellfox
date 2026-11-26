use crate::native::NativeResponse;
use anyhow::Result;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::oneshot;

type LocalApiReply =
    std::result::Result<Result<super::LocalApiResponse>, oneshot::error::RecvError>;
/// Legacy accepted wait keeps its canonical recipient token even when the
/// caller drops the receiver after owner admission. The reply arrives once
/// the changes its request made are on disk.
pub struct LocalApiAccepted {
    reply: Pin<Box<dyn Future<Output = LocalApiReply> + Send>>,
    _recipient: crate::grant::requests::RecipientCancellation,
}
impl LocalApiAccepted {
    pub(super) fn new(
        receiver: oneshot::Receiver<Result<super::LocalApiResponse>>,
        recipient: crate::grant::requests::RecipientCancellation,
        settled: impl Future<Output = ()> + Send + 'static,
    ) -> Self {
        Self {
            reply: Box::pin(async move {
                let reply = receiver.await?;
                settled.await;
                Ok(reply)
            }),
            _recipient: recipient,
        }
    }
}
impl Future for LocalApiAccepted {
    type Output = LocalApiReply;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().reply.as_mut().poll(cx)
    }
}
/// A request was admitted exactly once. Dropping this wait never retries it.
/// Remote helper control stays on its async manager, not the service owner.
pub enum NativeAccepted {
    Owner(Box<NativeOwnerReply>),
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
    /// Resolves once the changes the request made are on disk.
    settled: Pin<Box<dyn Future<Output = ()> + Send>>,
    /// The owner's answer, held until `settled`.
    answer: Option<Result<NativeResponse>>,
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
        settled: impl Future<Output = ()> + Send + 'static,
    ) -> Self {
        Self::Owner(Box::new(NativeOwnerReply {
            receiver,
            cancel,
            completed: false,
            settled: Box::pin(settled),
            answer: None,
        }))
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
            Self::Owner(reply) if reply.completed => match reply.settled.as_mut().poll(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(()) => Poll::Ready(reply.answer.take().unwrap_or_else(|| Err(anyhow::anyhow!("Service owner reply was already taken.")))),
            },
            Self::Owner(reply) => match Pin::new(&mut reply.receiver).poll(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(result)) => {
                    reply.completed = true;
                    reply.answer = Some(result);
                    match reply.settled.as_mut().poll(context) {
                        Poll::Pending => Poll::Pending,
                        Poll::Ready(()) => Poll::Ready(reply.answer.take().unwrap_or_else(|| Err(anyhow::anyhow!("Service owner reply was already taken.")))),
                    }
                }
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
