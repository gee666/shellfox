//! Authenticated experimental protocol over a private OS endpoint, never TCP.
#[path = "native_boot.rs"]
mod boot;
pub(crate) use boot::NativeControlChild;
#[path = "local_client.rs"]
mod client;
#[path = "local_connection.rs"]
mod connection;
#[path = "local_long_poll.rs"]
mod long_poll;
/// Held reads (one per attached terminal view or change feed) plus ordinary requests.
const MAX_CONNECTIONS: usize = 160;
#[path = "native_preparation.rs"]
mod preparation;
#[path = "local_transport_frame.rs"]
mod transport_frame;
use transport_frame::{Frame, LifetimeReply, TRANSPORT_VERSION};
#[path = "local_launch.rs"]
mod launch;
#[path = "local_request.rs"]
mod request;
#[path = "local_desktop.rs"]
mod desktop;
pub use desktop::{DesktopProviderRequest, DesktopReply, DesktopRequest};
#[path = "local_ssh.rs"]
pub(crate) mod ssh_read;
pub use ssh_read::{open_ssh_directory, read_ssh_profiles, ssh_endpoint_exists, SshRead, SshReadReply};
use crate::{
    experiment::{ExperimentRequest, ExperimentResponse, METHOD},
    platform,
    security::PrivateDirectory,
};
use anyhow::{bail, Context, Result};
pub use client::{HostClosed, LocalClient, LocalRequestFailure};

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
struct Envelope<T = ExperimentRequest> {
    client_id: uuid::Uuid,
    method: String,
    payload: T,
}
const NAMESPACE: &str = "shellfox.experimental.rust.v1";

#[path = "native_boot_api.rs"]
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
    let mut signal_error = None;
    let result = {
        let signal = async {
            if let Err(error) = stop_signal().await {
                signal_error = Some(error);
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
                status: boot.ready_status(),
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

struct ServerState {
    stop: shellfox_protocol::CancellationToken,
    tasks: tokio::task::JoinSet<Result<connection::ConnectionOutcome>>,
    lifetimes: tokio::task::JoinSet<()>,
    lifetime_slots: std::sync::Arc<tokio::sync::Semaphore>,
    lifetime_cancel: shellfox_protocol::CancellationToken,
}
impl ServerState {
    fn new() -> Self {
        Self {
            stop: shellfox_protocol::CancellationToken::new(),
            tasks: tokio::task::JoinSet::new(),
            lifetimes: tokio::task::JoinSet::new(),
            lifetime_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(128)),
            lifetime_cancel: shellfox_protocol::CancellationToken::new(),
        }
    }
    fn request_stop(&self) {
        self.stop.cancel();
    }
    fn ensure_running(&self) -> Result<()> {
        anyhow::ensure!(
            !self.stop.is_cancelled(),
            "Local host stop requested"
        );
        Ok(())
    }
    async fn drain(&mut self, client: Option<crate::OwnerClient>, alive: bool) {
        self.request_stop();
        // Keep all accepted response-consumption and opening-epoch bookkeeping,
        // including after unwind of the listener loop. Each connection has its deadline.
        while let Some(result) = self.tasks.join_next().await {
            if let Ok(Ok(connection::ConnectionOutcome::Lifetime(lifetime))) = result {
                self.lifetimes
                    .spawn(lifetime.watch(self.lifetime_cancel.clone()));
            }
        }
        self.lifetime_cancel.cancel();
        while self.lifetimes.join_next().await.is_some() {}
        if alive {
            if let Some(client) = client {
                // Best effort only. An exited actor cannot receive this command;
                // the following original actor ACK/join remains mandatory.
                let _ = client.close_all_lifetimes().await;
            }
        }
    }
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
    let polls = std::sync::Arc::new(long_poll::LongPolls::new(
        client.native_watches().await?,
        client.remote_state(),
    ));
    let lost = ownership.lost();
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
                    Some(Ok(Ok(connection::ConnectionOutcome::Lifetime(lifetime)))) => {
                        server.lifetimes.spawn(lifetime.watch(server.lifetime_cancel.clone()));
                    }
                    Some(Err(error)) => return Err(error.into()),
                    _ => {}
                }
            },
            accepted = listener.accept(), if server.tasks.len() < MAX_CONNECTIONS => {
                let stream = accepted?;
                server.tasks.spawn(connection::serve_connection(
                    stream, record.secret.clone(), client.clone(), lost.clone(),
                    server.stop.clone(), server.lifetime_slots.clone(), polls.clone(),
                ));
            },
            _ = server.stop.cancelled() => {}
            _ = lost.cancelled() => {}
        }
    }
    Ok(())
}

/// Ctrl+C, or the stop signal systemd, launchd and `kill` send to a headless host.
async fn stop_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (mut terminate, mut hangup) = (signal(SignalKind::terminate())?, signal(SignalKind::hangup())?);
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
            _ = hangup.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
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
