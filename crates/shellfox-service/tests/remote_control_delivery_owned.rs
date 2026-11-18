//! Accepted Manager work survives loss of its receipt before dequeue.
#[path = "support/native_bootstrap_owned.rs"]
pub mod support;
use anyhow::{ensure, Context, Result};
use shellfox_protocol::{AppError, CancellationToken, ErrorCode};
use shellfox_service::remote::preferences::RemotePreferences;
use shellfox_service::remote::runtime::{NetworkFuture, RemoteControlBackend};
use shellfox_service::remote::{RemoteNetwork, RemoteNetworkFactory, RemoteRequest, RemoteRuntime};
use shellfox_transport::{
    HelperConfig, IsolatedStateDirectory, PermissionVerifier, VerifiedHelper,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
fn denied() -> AppError {
    AppError {
        code: ErrorCode::NativeUnavailable,
        message: "Not used by controlled delivery test".into(),
        retryable: false,
    }
}
#[derive(Default)]
struct Backend {
    approvals: shellfox_remote::ApprovalControl,
    saves: AtomicUsize,
    last: Mutex<Option<RemotePreferences>>,
}
impl RemoteControlBackend for Backend {
    fn preferences(&self) -> NetworkFuture<'_, Option<RemotePreferences>> {
        Box::pin(async { Ok(None) })
    }
    fn save_preferences(&self, value: RemotePreferences) -> NetworkFuture<'_, ()> {
        Box::pin(async move {
            self.saves.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().expect("last prefs") = Some(value);
            Ok(())
        })
    }
    fn approvals(&self) -> shellfox_remote::ApprovalControl {
        self.approvals.clone()
    }
    fn helper_config(
        &self,
        _: IsolatedStateDirectory,
        _: String,
    ) -> std::result::Result<HelperConfig, AppError> {
        Err(denied())
    }
    fn approve_full_account<'a>(&'a self, _: &'a str) -> NetworkFuture<'a, ()> {
        Box::pin(async { Err(denied()) })
    }
    fn revoke<'a>(&'a self, _: &'a str) -> NetworkFuture<'a, ()> {
        Box::pin(async { Err(denied()) })
    }
}
struct NoNetwork;
impl RemoteNetworkFactory for NoNetwork {
    fn start(&self, _: HelperConfig) -> NetworkFuture<'_, Arc<dyn RemoteNetwork>> {
        panic!("Disabled controlled mutation must not start a node")
    }
}
#[test]
fn accepted_mutation_dropped_before_dequeue_executes_original_backend_once() -> Result<()> {
    let mut fixture = support::Fixture::new()?;
    let rt = support::runtime()?;
    let verifier =
        PermissionVerifier::new(VerifiedHelper::new(fixture.helper.clone(), fixture.digest)?);
    let directory = rt.block_on(IsolatedStateDirectory::create_new_private(
        fixture.data("accepted-state"),
        &verifier,
    ))?;
    let backend = Arc::new(Backend::default());
    let (control, mut owner) = rt
        .block_on(RemoteRuntime::spawn(
            backend.clone(),
            directory,
            Arc::new(NoNetwork),
            "original-controller".into(),
            CancellationToken::new(),
        ))
        .map_err(|e| anyhow::anyhow!(e.message))?;
    // The current-thread Runtime is now STOPPED. Tokio spawn never polls the
    // Manager synchronously; it cannot dequeue until the explicit release below.
    let accepted = control
        .try_submit(RemoteRequest::Configure {
            enabled: false,
            hostname: "accepted-once".into(),
        })
        .map_err(|_| anyhow::anyhow!("Valid mutation not enqueued"))?;
    drop(accepted);
    let after = control
        .try_submit(RemoteRequest::State)
        .map_err(|_| anyhow::anyhow!("FIFO observation not enqueued"))?;
    ensure!(
        backend.saves.load(Ordering::SeqCst) == 0,
        "Dequeue gate was not closed"
    );
    // Release the SAME scheduler/Manager. This FIFO State receipt cannot arrive
    // before the preceding mutation, even though its own receiver was dropped.
    let observed = rt
        .block_on(after)
        .context("Original Manager response lost")
        .and_then(|reply| reply.map_err(|e| anyhow::anyhow!(e.message)));
    let stopped = rt
        .block_on(owner.shutdown())
        .map_err(|e| anyhow::anyhow!(e.message));
    stopped?;
    let state = observed?;
    ensure!(
        state.hostname == "accepted-once" && backend.saves.load(Ordering::SeqCst) == 1,
        "Accepted mutation was skipped or repeated"
    );
    ensure!(backend
        .last
        .lock()
        .expect("last prefs")
        .as_ref()
        .is_some_and(|p| p.hostname == "accepted-once"));
    ensure!(
        backend.saves.load(Ordering::SeqCst) == 1,
        "Shutdown replayed accepted work"
    );
    fixture.success();
    Ok(())
}
