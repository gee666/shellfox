//! One service owner thread. Async permissions, loss supervision and deadlines
//! remain runnable while a PTY startup or confirmed cleanup is waiting.
use crate::{
    experiment::{ExperimentRequest, ExperimentResponse},
    OwnerDispatcher, Service,
};
use anyhow::{bail, Context, Result};
use shellfox_protocol::CancellationToken;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, SyncSender, TrySendError},
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::oneshot;

#[cfg(test)]
#[path = "owner_lifetime_tests.rs"]
mod lifetime_tests;
#[cfg(test)]
#[path = "owner_private_tests.rs"]
mod private_tests;
#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;

#[path = "native_boot_actor.rs"]
mod cleanup;
#[path = "owner_client.rs"]
mod client;
#[path = "owner_controller.rs"]
mod controller;
use controller::controller_id;
#[path = "owner_task.rs"]
mod task;
use task::Task;
#[path = "owner_approvals.rs"]
pub(crate) mod approvals;
#[path = "owner_native.rs"]
mod native_reply;
#[path = "owner_private.rs"]
mod private;
#[path = "owner_private_client.rs"]
mod private_client;
#[path = "owner_start.rs"]
mod start;
#[path = "owner_wake.rs"]
mod wake;
pub use wake::{OwnerWake, OwnerWakeSlot};
pub(crate) use cleanup::report_warning;
use cleanup::StartupFailure;
pub(crate) use start::StartError;
pub use native_reply::{ExperimentAccepted, NativeAccepted};

const CAPACITY: usize = 16;
#[derive(Clone)]
pub struct OwnerClient {
    sender: SyncSender<Task>,
    client_id: uuid::Uuid,
    remote: Option<crate::remote::native::RemoteControl>,
}
pub(crate) struct ServiceOwner {
    sender: SyncSender<Task>,
    started: Option<oneshot::Receiver<Result<()>>>,
    done: oneshot::Receiver<crate::ShutdownOutcome>,
    thread: Option<JoinHandle<()>>,
    abandon: CancellationToken,
    cleanup_done: Option<crate::ShutdownOutcome>,
    cleanup_error: Option<String>,
}
impl ServiceOwner {
    fn spawn_inner<D: OwnerDispatcher + Send + 'static>(
        directory: PathBuf,
        supervisor: PathBuf,
        lost: CancellationToken,
        lifetime: Option<shellfox_transport::RootLifetimeGuard>,
        mut dispatcher: D,
    ) -> Result<Self> {
        let (sender, incoming) = mpsc::sync_channel(CAPACITY);
        let (ready, started) = oneshot::channel();
        let (finished, done) = oneshot::channel();
        let abandon = CancellationToken::new();
        let owner_abandon = abandon.clone();
        let wake = OwnerWake::new(sender.clone());
        let runtime_handle = tokio::runtime::Handle::try_current().ok();
        let thread = std::thread::Builder::new()
            .name("shellfox-service-owner".into())
            .spawn(move || {
                if lost.is_cancelled() {
                    let _ = ready.send(Err(anyhow::anyhow!(
                        "Native ownership lost before service startup."
                    )));
                    let _ = finished.send(cleanup::committed());
                    return;
                }
                let mut service =
                    match Self::open_for_actor(&directory, &supervisor, &lost, lifetime, owner_notifier(&wake)) {
                        Ok(service) => service,
                        Err(failure) => {
                            let _ = ready.send(Err(failure.cause));
                            let _ = finished.send(failure.shutdown);
                            return;
                        }
                    };
                if lost.is_cancelled() {
                    let _ = ready.send(Err(anyhow::anyhow!(
                        "Native ownership lost during service startup."
                    )));
                    let _ = finished.send(cleanup::finish(service));
                    return;
                }
                if ready.send(Ok(())).is_err() {
                    let _ = finished.send(cleanup::finish(service));
                    return;
                }
                service.runtime_handle = runtime_handle;
                let mut active = false;
                let mut remote_install: Option<std::sync::Arc<approvals::RemoteInstall>> = None;
                let mut controllers = HashMap::new();
                let mut private = private::PrivateOwner::new();
                dispatcher.bind_wake(wake.clone());
                loop {
                    if lost.is_cancelled() || owner_abandon.is_cancelled() {
                        break;
                    }
                    wake.rearm();
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        service.pump_power();
                        service.pump();
                    }))
                    .is_err()
                    {
                        // Retain the Service and original blocking-worker handles
                        // through the same cleanup path as dispatcher failures.
                        break;
                    }
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        private.pump(&mut service)
                    }))
                    .is_err()
                    {
                        break;
                    }
                    if active
                        && !service.stopping
                        && !std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if let Some(remote) = &remote_install {
                                remote.dispatch_one(&mut service).is_ok()
                            } else {
                                dispatcher.dispatch_one(&mut service);
                                true
                            }
                        }))
                        .unwrap_or(false)
                    {
                        break;
                    }
                    match incoming.recv_timeout(service.owner_pass_timeout()) {
                        Ok(Task::Approvals(task)) => approvals::handle(&mut service, *task, &lost),
                        Ok(Task::InstallRemote(install)) => {
                            if !active && remote_install.is_none() {
                                install.bind_wake(wake.clone());
                                install.acknowledge(true);
                                remote_install = Some(install);
                            } else {
                                install.acknowledge(false);
                            }
                        }
                        Ok(Task::Activate { reply }) => {
                            active = true;
                            let _ = reply.send(());
                        }
                        Ok(Task::Execute {
                            cancel,
                            request,
                            client_id,
                            reply,
                        }) => {
                            if lost.is_cancelled() {
                                let _ = reply.send(Err(anyhow::anyhow!("Native ownership lost.")));
                                break;
                            }
                            let controller =
                                match controller_id(&mut controllers, &mut service, client_id) {
                                    Ok(controller) => controller,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                            let detach = matches!(request, ExperimentRequest::Detach {});
                            // Complete metadata mutations; only unresolved ownership
                            // offers follow recipient cancellation into exact cleanup.
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                service.execute_for_recipient(request, controller, cancel)
                            })) {
                                Ok(result) => {
                                    if detach && result.is_ok() {
                                        controllers.remove(&client_id);
                                    }
                                    let _ = reply.send(result);
                                }
                                Err(_) => {
                                    let _ = reply.send(Err(anyhow::anyhow!(
                                        "Service owner operation panicked; cleanup required."
                                    )));
                                    break;
                                }
                            }
                        }
                        Ok(Task::Private(task)) => {
                            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                private.handle(&mut service, *task, &mut controllers)
                            }))
                            .is_err()
                            {
                                break;
                            }
                        }
                        Ok(Task::Native {
                            cancel,
                            action,
                            client_id,
                            reply,
                        }) => {
                            if lost.is_cancelled() {
                                let _ = reply.send(Err(anyhow::anyhow!("Native ownership lost.")));
                                break;
                            }
                            let controller =
                                match controller_id(&mut controllers, &mut service, client_id) {
                                    Ok(controller) => controller,
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                        continue;
                                    }
                                };
                            if let crate::native::NativeAction::Power(request) = *action {
                                service.request_power(request, cancel, crate::power_service::PowerReply::Native(reply));
                                continue;
                            }
                            if matches!(*action, crate::native::NativeAction::TabStats) {
                                service.enqueue_tab_stats(crate::tab_stats::StatsReply::Native(reply));
                                continue;
                            }
                            if matches!(*action, crate::native::NativeAction::TerminalProfiles) {
                                service.enqueue_native_catalog(controller, reply);
                                continue;
                            }
                            if matches!(*action, crate::native::NativeAction::System(_)) {
                                if let Err(error) = service.admit_system_native(controller) {
                                    let _ = reply.send(Err(anyhow::anyhow!(error.message)));
                                    continue;
                                }
                            }
                            let detach = matches!(*action, crate::native::NativeAction::DetachAll);
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                service.with_grant_recipient(cancel, |service| {
                                    service.execute_native(*action, controller)
                                })
                            })) {
                                Ok(result) => {
                                    if detach && result.is_ok() {
                                        controllers.remove(&client_id);
                                    }
                                    let _ = reply.send(result);
                                }
                                Err(_) => {
                                    let _ = reply.send(Err(anyhow::anyhow!(
                                        "Native owner operation panicked; cleanup required."
                                    )));
                                    break;
                                }
                            }
                        }
                        Ok(Task::SourceFactsWatch { reply }) => {
                            let _ = reply.send(service.source_facts_watch());
                        }
                        Ok(Task::Watches { reply }) => {
                            let _ = reply.send(service.native_watches());
                        }
                        Ok(Task::Wake) => {}
                        Ok(Task::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                let _ = finished.send(cleanup::finish(service));
            })?;
        Ok(Self {
            sender,
            started: Some(started),
            done,
            thread: Some(thread),
            abandon,
            cleanup_done: None,
            cleanup_error: None,
        })
    }
    pub async fn activate(&self) -> Result<()> {
        let (reply, ready) = oneshot::channel();
        self.sender
            .try_send(Task::Activate { reply })
            .map_err(|_| anyhow::anyhow!("Service owner activation failed."))?;
        ready
            .await
            .context("Service owner stopped before activation was confirmed.")
    }
    pub fn client(&self, client_id: uuid::Uuid) -> OwnerClient {
        OwnerClient {
            sender: self.sender.clone(),
            client_id,
            remote: None,
        }
    }
    pub async fn shutdown(&mut self) -> Result<crate::ShutdownOutcome> {
        // The borrowed controller remains in the boot guard on every failure.
        self.abandon.cancel();
        let _ = self.sender.try_send(Task::Shutdown);
        if let Some(error) = &self.cleanup_error {
            bail!("{error}");
        }
        if self.cleanup_done.is_none() {
            match (&mut self.done).await {
                Ok(outcome) => self.cleanup_done = Some(outcome),
                Err(_) => {
                    self.cleanup_error = Some("Service owner cleanup acknowledgement lost.".into())
                }
            }
        }
        if let Some(thread) = self.thread.take() {
            let joined = tokio::task::spawn_blocking(move || thread.join()).await;
            if !matches!(joined, Ok(Ok(()))) {
                self.cleanup_error = Some("Service owner panicked before confirmed join.".into());
            }
        }
        if let Some(error) = &self.cleanup_error {
            bail!("{error}");
        }
        self.cleanup_done
            .clone()
            .context("Service owner cleanup remains unconfirmed.")
    }
}

fn owner_notifier(wake: &OwnerWake) -> shellfox_runtime::EventWake {
    let wake = wake.clone();
    std::sync::Arc::new(move || wake.notify())
}

impl Drop for ServiceOwner {
    fn drop(&mut self) {
        self.abandon.cancel();
    }
}
