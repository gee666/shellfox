use super::*;
use crate::OwnerClient;
use shellfox_protocol::CancellationToken;
use std::sync::Arc;
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
    pub async fn watch(mut self, cancel: CancellationToken, orderly: CancellationToken) {
        let mut byte = [0; 1];
        tokio::select! {
            _ = self.stream.read(&mut byte) => {} // EOF/error or forbidden data: release this epoch.
            _ = cancel.cancelled() => if orderly.is_cancelled() {
                // An orderly stop says so before closing; a failure only closes.
                let _ = tokio::time::timeout(Duration::from_secs(1), async {
                    self.stream.write_all(&[super::transport_frame::LIFETIME_STOPPING]).await?;
                    self.stream.flush().await
                })
                .await;
            }
        }
        // A lost owner's global cleanup owns the remaining release.
        tokio::select! {
            _ = self.client.close_lifetime(self.epoch) => {}
            _ = self.lost.cancelled() => {}
        }
    }
}
/// What every accepted connection shares with the server loop.
#[derive(Clone)]
pub(super) struct Shared {
    pub secret: String,
    pub client: OwnerClient,
    pub lost: CancellationToken,
    pub stop: CancellationToken,
    pub orderly: CancellationToken,
    pub slots: Arc<Semaphore>,
    pub polls: Arc<super::long_poll::LongPolls>,
}
pub(super) async fn serve_connection(
    mut stream: platform::LocalStream,
    shared: Shared,
) -> Result<ConnectionOutcome> {
    let Shared {
        secret,
        client,
        lost,
        stop,
        orderly,
        slots,
        polls,
    } = shared;
    let frame = tokio::time::timeout(Duration::from_secs(30), async {
        let bytes = read_frame(&mut stream, crate::remote::outgoing_wire::MAX_REQUEST * 5).await?;
        stream.verify_peer()?;
        let frame: Frame = serde_json::from_slice(&bytes)?;
        frame.authenticate(&secret)?;
        if lost.is_cancelled() || stop.is_cancelled() {
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
                let outgoing = matches!(&request, PrivateRequest::Peer(_));
                let asynchronous = outgoing
                    || matches!(
                        &request,
                        PrivateRequest::RemoteOperator(_) | PrivateRequest::DesktopView(_)
                        | PrivateRequest::Power(crate::power::PowerRequest::Status {})
                        // Held reads end when their caller goes away.
                        | PrivateRequest::LocalApi(crate::local_api::LocalApiRequest::Poll { .. })
                        | PrivateRequest::DesktopLaunch(
                            crate::desktop_launch::DesktopLaunchRequest::PollAttached { .. }
                                | crate::desktop_launch::DesktopLaunchRequest::Receipt { .. }
                        )
                    );
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
                let output_read = super::long_poll::LongPolls::output_read(&request);
                let operation = async {
                    // Reads wait here, per connection, until they can return something new.
                    polls.hold(client_id, &request, &stop).await?;
                    original.private_request_in_epoch(request, epoch).await
                };
                let result = if asynchronous {
                    let mut byte = [0; 1];
                    tokio::select! {
                        result=operation=>result,
                        _=stream.read(&mut byte)=>bail!("Asynchronous private caller closed or sent forbidden data"),
                    }
                } else {
                    operation.await
                };
                if let (Some(read), Ok(value)) = (output_read, &result) {
                    polls.answered_output(client_id, read, value);
                }
                let reply = match result {
                    Ok(value) => {
                        if requested_stop {
                            orderly.cancel();
                            stop.cancel();
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
    let (code, message) =
        super::runtime_codes::to_wire(&error).unwrap_or_else(|| failure_code(error));
    shellfox_protocol::Reply::Failure(shellfox_protocol::AppError {
        code,
        message,
        retryable: false,
    })
}
fn failure_code(error: anyhow::Error) -> (shellfox_protocol::ErrorCode, String) {
    match error.downcast_ref::<crate::ShutdownFailure>() {
        Some(
            failure @ (crate::ShutdownFailure::ActivityPersistence(_)
            | crate::ShutdownFailure::MetadataPersistence(_)),
        ) => (
            shellfox_protocol::ErrorCode::StorageFailed,
            failure.to_string(),
        ),
        Some(failure @ crate::ShutdownFailure::ResourceCleanup(_)) => (
            shellfox_protocol::ErrorCode::NativeUnavailable,
            failure.to_string(),
        ),
        None if error.is::<shellfox_terminal::raw_process::AutomationStdinClosed>() => (shellfox_protocol::ErrorCode::TargetLost,"Automation stdin is closed.".into()),
        None if error.is::<crate::AutomationViewFailure>() => (shellfox_protocol::ErrorCode::NativeUnavailable, "Original raw process cleanup is confirmed, but final automation rendering failed; source evidence is retained.".into()),
        None if error.is::<crate::owner::launching::LaunchFailure>() => (shellfox_protocol::ErrorCode::LaunchFailed, error.to_string()),
        None if error.is::<shellfox_runtime::ExternalStartError>() => (shellfox_protocol::ErrorCode::NativeUnavailable, "External parser startup was not ready; no raw target was launched and any pending original owner is retained for confirmed cleanup.".into()),
        None => {
            // The client only gets the generic reply; the host's own log keeps the cause.
            eprintln!("Shellfox host: a local request failed: {error:#}");
            (
            shellfox_protocol::ErrorCode::Internal,
            "Private host operation failed; metadata and unconfirmed ownership retained.".into(),
            )
        }
    }
}
