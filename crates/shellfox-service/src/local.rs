//! Authenticated local API over a private OS endpoint, never TCP.
mod boot;
pub(crate) use boot::NativeControlChild;
mod client;
mod server;
use server::{stop_signal, ServerState};
mod connection;
mod long_poll;
/// Held reads (one per attached terminal view or change feed) plus ordinary requests.
const MAX_CONNECTIONS: usize = 160;
mod preparation;
mod transport_frame;
use transport_frame::{Frame, LifetimeReply, TRANSPORT_VERSION};
mod desktop;
mod launch;
mod request;
mod runtime_codes;
pub use desktop::{DesktopProviderRequest, DesktopReply, DesktopRequest};
pub(crate) mod ssh_read;
use crate::{
    local_api::{LocalApiRequest, LocalApiResponse, METHOD},
    platform,
    security::PrivateDirectory,
};
use anyhow::{bail, Context, Result};
pub use client::{HostClosed, LocalClient, LocalRequestFailure, OtherBuildRunning};
pub use ssh_read::{
    open_ssh_directory, read_ssh_profiles, ssh_endpoint_exists, SshRead, SshReadReply,
};

pub async fn check_helper_protocol(helper: &std::path::Path, digest: [u8; 32]) -> Result<()> {
    let helper = shellfox_transport::VerifiedHelper::new(helper.to_owned(), digest)?;
    shellfox_transport::HelperTransport::probe(&helper).await?;
    Ok(())
}
pub(crate) use request::PrivateRequest;
use serde::{Deserialize, Serialize};
pub use shellfox_protocol::{automation, decode_chunk, encode_chunk};
use shellfox_protocol::{
    LocalTransport as LocalTransportContract, RequestLimits, TransportError, TransportFuture,
    Validate,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    namespace: String,
    address: String,
    secret: String,
    #[serde(rename = "transportVersion")]
    transport_version: u8,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope<T = LocalApiRequest> {
    client_id: uuid::Uuid,
    method: String,
    payload: T,
}
// Never change the endpoint record namespace, the transport version or the
// shellfox_protocol local namespaces: a newer desktop uses them to find and
// replace an older build's host. Pinned by tests::wire_names_are_frozen.
const NAMESPACE: &str = "shellfox.local.endpoint.v1";

mod api;
pub use api::{
    serve, serve_remote_with_boot_report, serve_remote_with_boot_status, serve_with_boot_report,
    serve_with_boot_status, serve_with_dispatcher, serve_with_ready, LocalBootReport,
    LocalBootStatus, LocalBootStop, LocalBootstrap, OccupiedOwnerError, RemoteBootOptions,
    SourceFactsInstaller,
};

async fn serve_boot_plan<D: crate::OwnerDispatcher + 'static>(
    data: &Path,
    helper: &Path,
    helper_sha256: [u8; 32],
    dispatcher: D,
    remote: Option<RemoteBootOptions>,
    ready: impl FnOnce(crate::OwnerClient),
    report: impl Fn(LocalBootReport),
) -> Result<()> {
    let mut bootstrap = match LocalBootstrap::plan(data, helper, helper_sha256) {
        Ok(bootstrap) => bootstrap,
        Err(error) => {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                report(LocalBootReport {
                    status: LocalBootStatus::NoOwnerClean,
                    shutdown: Some(crate::ShutdownOutcome {
                        activity: crate::ActivityShutdownState::Committed,
                        presentation: crate::PresentationShutdownState::NoKnownFailure,
                    }),
                    endpoint: None,
                })
            }));
            return Err(error);
        }
    };
    serve_retained_plan(&mut bootstrap.boot, dispatcher, remote, ready, report).await
}

async fn serve_retained_plan<D: crate::OwnerDispatcher + 'static>(
    boot: &mut boot::BootGuard,
    dispatcher: D,
    remote: Option<RemoteBootOptions>,
    ready: impl FnOnce(crate::OwnerClient),
    report: impl Fn(LocalBootReport),
) -> Result<()> {
    let report_cb = |value| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| report(value)));
    };
    let progress = |status| {
        if status != LocalBootStatus::NoOwnerClean {
            report_cb(LocalBootReport {
                status,
                shutdown: None,
                endpoint: None,
            });
        }
    };
    progress(LocalBootStatus::RetainedCleanup);
    let stop = boot.server.stop.clone();
    let orderly = boot.server.orderly.clone();
    let mut signal_error = None;
    let result = {
        let signal = async {
            match stop_signal().await {
                Ok(()) => orderly.cancel(),
                Err(error) => signal_error = Some(error),
            }
            stop.cancel();
            std::future::pending::<()>().await;
        };
        let work = async {
            boot.server.ensure_running()?;
            match boot
                .preparation
                .acquire(&mut boot.private, &boot.server.stop)
                .await?
            {
                shellfox_transport::OwnershipDecision::Acquired => {}
                shellfox_transport::OwnershipDecision::Occupied(proof) => {
                    let directory = boot
                        .private
                        .path
                        .parent()
                        .context("Missing bound root")?
                        .to_owned();
                    return Err(OccupiedOwnerError::proved(
                        directory,
                        proof,
                        boot.private.creation_owner(),
                    )
                    .into());
                }
            }
            boot.server.ensure_running()?;
            if let Some(remote) = remote {
                boot.start_remote(remote).await?;
            } else {
                boot.start(dispatcher).await?;
            }
            boot.server.ensure_running()?;
            report_cb(LocalBootReport {
                status: LocalBootStatus::Ready,
                shutdown: None,
                endpoint: Some(boot.endpoint_report()),
            });
            run_started(boot, ready).await
        };
        tokio::pin!(signal, work);
        // Register Ctrl-C before startup. A signal requests closing, never drops
        // the in-flight helper/start future or substitutes for its cleanup.
        tokio::select! {
            biased;
            _ = &mut signal => unreachable!("signal remains live after requesting stop"),
            result = &mut work => result,
        }
    };
    let result = result.and_then(|()| signal_error.map_or(Ok(()), |error| Err(error.into())));
    let shutdown = boot.cleanup(&progress).await;
    report_cb(LocalBootReport {
        status: LocalBootStatus::NoOwnerClean,
        shutdown: Some(shutdown),
        endpoint: Some(boot.endpoint_report()),
    });
    result.map_err(|error| preparation::retain_error(error, &boot.private))
}

async fn run_started(
    boot: &mut boot::BootGuard,
    ready: impl FnOnce(crate::OwnerClient),
) -> Result<()> {
    let remote_control = boot.remote_control.clone();
    let service = boot
        .actor
        .as_ref()
        .context("Service owner missing after startup.")?;
    let ownership = boot.preparation.acquired_view()?;
    let listener = boot
        .listener
        .as_mut()
        .context("Listener missing after startup.")?;
    let record = boot
        .published
        .as_ref()
        .context("Published capability missing after startup.")?;
    let server = &mut boot.server;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let client = service.client(uuid::Uuid::new_v4());
        ready(if let Some(remote) = &remote_control {
            client.with_remote(remote.clone())
        } else {
            client
        })
    }))
    .map_err(|_| anyhow::anyhow!("Embedded host readiness callback failed."))?;
    let client = service.client(uuid::Uuid::new_v4());
    let client = match remote_control {
        Some(remote) => client.with_remote(remote),
        None => client,
    };
    let lost = ownership.lost();
    // The owner may be busy; a stop or lost ownership must not wait for it.
    let watches = tokio::select! {
        biased;
        _ = server.stop.cancelled() => return Ok(()),
        _ = lost.cancelled() => anyhow::bail!("Native host ownership lost; refusing further metadata access."),
        watches = client.native_watches() => watches?,
    };
    let polls = std::sync::Arc::new(long_poll::LongPolls::new(watches, client.remote_state()));
    let shared = connection::Shared {
        secret: record.secret.clone(),
        client: client.clone(),
        lost: lost.clone(),
        stop: server.stop.clone(),
        orderly: server.orderly.clone(),
        slots: server.lifetime_slots.clone(),
        polls: polls.clone(),
    };
    while server.ensure_running().is_ok() {
        anyhow::ensure!(
            ownership.is_alive(),
            "Native host ownership lost; refusing further metadata access."
        );
        tokio::select! {
            biased;
            result = server.lifetimes.join_next(), if !server.lifetimes.is_empty() => {
                anyhow::ensure!(!result.is_some_and(|result| result.is_err()), "Private lifetime task panicked; owner cleanup required.");
            },
            result = server.tasks.join_next(), if !server.tasks.is_empty() => {
                match result {
                    Some(Ok(outcome)) => server.adopt(outcome),
                    Some(Err(error)) => return Err(error.into()),
                    _ => {}
                }
            },
            accepted = listener.accept(), if server.tasks.len() < MAX_CONNECTIONS => {
                let stream = accepted?;
                server.tasks.spawn(connection::serve_connection(stream, shared.clone()));
            },
            _ = server.stop.cancelled() => {}
            _ = lost.cancelled() => {}
        }
    }
    Ok(())
}

fn preflight_boot_paths(data: &Path, helper: &Path) -> Result<()> {
    if !helper.is_absolute() || !helper.is_file() {
        bail!("Explicit absolute host data path and existing helper executable required.");
    }
    crate::security::HostRoot::inspect(data)?;
    Ok(())
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin>(
    read: &mut R,
    maximum: usize,
) -> Result<Vec<u8>> {
    let length = read.read_u32().await? as usize;
    if length == 0 || length > maximum {
        bail!("Invalid local frame length.");
    }
    let mut bytes = vec![0; length];
    read.read_exact(&mut bytes).await?;
    Ok(bytes)
}
pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(write: &mut W, bytes: &[u8]) -> Result<()> {
    write.write_u32(u32::try_from(bytes.len())?).await?;
    write.write_all(bytes).await?;
    write.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn wire_names_are_frozen() {
        assert_eq!(super::NAMESPACE, "shellfox.local.endpoint.v1");
        assert_eq!(super::TRANSPORT_VERSION, 1);
        assert_eq!(shellfox_protocol::LOCAL_NAMESPACE, "private.local.v1");
        assert_eq!(
            shellfox_protocol::DESKTOP_LAUNCH_NAMESPACE,
            "private.desktop.launch.v1"
        );
        assert_eq!(
            shellfox_protocol::DESKTOP_VIEW_NAMESPACE,
            "private.desktop.view.v1"
        );
        assert_eq!(shellfox_protocol::POWER_NAMESPACE, "private.power.v1");
        assert_eq!(shellfox_protocol::SSH_READ_NAMESPACE, "private.ssh.read.v1");
    }
}
