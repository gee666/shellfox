use super::*;
use crate::OwnerClient;
use shellfox_protocol::CancellationToken;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) enum ConnectionOutcome {
    Complete,
    Lifetime(LifetimeConnection),
}
pub(super) struct LifetimeConnection {
    stream: platform::LocalStream,
    client: OwnerClient,
    epoch: uuid::Uuid,
    lost: CancellationToken,
    _permit: OwnedSemaphorePermit,
}
impl LifetimeConnection {
    pub async fn watch(mut self, cancel: CancellationToken) {
        let mut byte = [0; 1];
        loop {
            tokio::select! {
                _=self.stream.read(&mut byte)=>break, // EOF/error or forbidden data: release this epoch.
                _=tokio::time::sleep(Duration::from_millis(20))=>if cancel.is_cancelled(){break;},
            }
        }
        let release = self.client.close_lifetime(self.epoch);
        tokio::pin!(release);
        loop {
            tokio::select! {
                _=&mut release=>break,
                _=tokio::time::sleep(Duration::from_millis(20))=>if self.lost.is_cancelled(){break;},
            }
        } // Lost owner's original global cleanup owns the remaining release.
    }
}
pub(super) async fn serve_connection(
    mut stream: platform::LocalStream,
    secret: String,
    client: OwnerClient,
    lost: CancellationToken,
    stop: Arc<AtomicBool>,
    slots: Arc<Semaphore>,
) -> Result<ConnectionOutcome> {
    let frame = tokio::time::timeout(Duration::from_secs(30), async {
        let bytes = read_frame(&mut stream, crate::remote::outgoing_wire::MAX_REQUEST * 5).await?;
        stream.verify_peer()?;
        let frame: Frame = serde_json::from_slice(&bytes)?;
        frame.authenticate(&secret)?;
        if lost.is_cancelled() || stop.load(Ordering::Acquire) {
            bail!("Host admission is closed.");
        }
        Ok::<_, anyhow::Error>(frame)
    })
    .await
    .context("Private transport header deadline.")??;
    match frame {
        Frame::LifetimeOpen { client_id, .. } => {
            let permit = slots
                .try_acquire_owned()
                .context("Private lifetime capacity reached.")?;
            let client = client.for_private_client(client_id);
            let epoch = client.open_lifetime(uuid::Uuid::new_v4()).await?;
            let opening = tokio::time::timeout(Duration::from_secs(30), async {
                let reply = LifetimeReply::Opened {
                    version: TRANSPORT_VERSION,
                    client_id,
                    epoch,
                };
                write_frame(&mut stream, &serde_json::to_vec(&reply)?).await?;
                response_consumed(&mut stream).await
            })
            .await
            .context("Private lifetime response deadline.")
            .and_then(|result| result);
            if let Err(error) = opening {
                client.close_lifetime(epoch).await?;
                return Err(error);
            }
            Ok(ConnectionOutcome::Lifetime(LifetimeConnection {
                stream,
                client,
                epoch,
                lost,
                _permit: permit,
            }))
        }
        Frame::Request {
            client_id,
            epoch,
            body,
            ..
        } => {
            let operation = async {
                if body.len() > crate::remote::outgoing_wire::MAX_REQUEST || epoch.is_nil() {
                    bail!("Invalid private request bound/epoch.");
                }
                let (inner, request) = PrivateRequest::decode(&body)?;
                if inner != client_id {
                    bail!("Private envelope/lifetime identity mismatch.");
                }
                let outgoing = matches!(&request, PrivateRequest::OutgoingV2(_));
                let asynchronous =
                    outgoing || matches!(&request, PrivateRequest::RemoteOperator(_));
                if !outgoing && body.len() > shellfox_protocol::MAX_REQUEST_BYTES {
                    bail!("Ordinary private request exceeds bound");
                }
                let maximum = if outgoing {
                    crate::remote::outgoing_wire::MAX_RESPONSE
                } else {
                    shellfox_protocol::MAX_RESPONSE_BYTES
                };
                let requested_stop = request.requested_stop();
                let original = client.for_private_client(client_id);
                let operation = original.private_request_in_epoch(request, epoch);
                let result = if asynchronous {
                    let mut byte = [0; 1];
                    tokio::select! {
                        result=operation=>result,
                        _=stream.read(&mut byte)=>bail!("Asynchronous private caller closed or sent forbidden data"),
                    }
                } else {
                    operation.await
                };
                let reply = match result {
                    Ok(value) => {
                        if requested_stop {
                            stop.store(true, Ordering::Release);
                        }
                        shellfox_protocol::Reply::Success(value)
                    }
                    Err(error) => failure(error),
                };
                let bytes = serde_json::to_vec(&reply)?;
                if bytes.len() > maximum {
                    bail!("Response exceeds bound.");
                }
                write_frame(&mut stream, &bytes).await?;
                response_consumed(&mut stream).await?;
                stream.shutdown().await?;
                Ok(ConnectionOutcome::Complete)
            };
            tokio::time::timeout(Duration::from_secs(30), operation)
                .await
                .context("Private request deadline expired; admitted mutation was not retried.")?
        }
    }
}
async fn response_consumed(stream: &mut platform::LocalStream) -> Result<()> {
    let receipt = *uuid::Uuid::new_v4().as_bytes();
    stream.write_all(&receipt).await?;
    stream.flush().await?;
    let mut received = [0; 16];
    stream.read_exact(&mut received).await?;
    if !bool::from(received.ct_eq(&receipt)) {
        bail!("Invalid local response receipt.");
    }
    Ok(())
}
fn failure(error: anyhow::Error) -> shellfox_protocol::Reply<serde_json::Value> {
    let (code, message) = match error.downcast_ref::<crate::ShutdownFailure>() {
        Some(failure @ crate::ShutdownFailure::ActivityPersistence(_)) => (
            shellfox_protocol::ErrorCode::StorageFailed,
            failure.to_string(),
        ),
        Some(failure @ crate::ShutdownFailure::ResourceCleanup(_)) => (
            shellfox_protocol::ErrorCode::NativeUnavailable,
            failure.to_string(),
        ),
        None if error.is::<shellfox_terminal::raw_process::AutomationStdinClosed>() => (shellfox_protocol::ErrorCode::TargetLost,"Automation stdin is closed.".into()),
        None if error.is::<crate::AutomationViewFailure>() => (shellfox_protocol::ErrorCode::NativeUnavailable, "Original raw process cleanup is confirmed, but final automation rendering failed; source evidence is retained.".into()),
        None if error.is::<shellfox_runtime::ExternalStartError>() => (shellfox_protocol::ErrorCode::NativeUnavailable, "External parser startup was not ready; no raw target was launched and any pending original owner is retained for confirmed cleanup.".into()),
        None => (
            shellfox_protocol::ErrorCode::Internal,
            "Private host operation failed; metadata and unconfirmed ownership retained.".into(),
        ),
    };
    shellfox_protocol::Reply::Failure(shellfox_protocol::AppError {
        code,
        message,
        retryable: false,
    })
}
