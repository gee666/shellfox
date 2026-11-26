//! One service owner thread. Async permissions, loss supervision and deadlines
//! remain runnable while a PTY startup or confirmed cleanup is waiting.
pub(crate) mod wait;
use crate::{
    local_api::{LocalApiRequest, LocalApiResponse},
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

/// Tasks held behind a close at most; the inbox holds the rest.
const MAX_DEFERRED: usize = 64;

#[cfg(test)]
mod lifetime_tests;
#[cfg(test)]
mod private_tests;
#[cfg(test)]
mod tests;

mod cleanup;
mod client;
mod health;
pub(crate) use health::{OwnerHealth, Step, SLOW_PASS};
mod controller;
use controller::controller_id;
mod task;
use task::Task;
pub(crate) mod approvals;
pub(crate) mod launching;
mod native_reply;
mod private;
mod private_client;
mod start;
mod wake;
pub(crate) use cleanup::report_warning;
use cleanup::StartupFailure;
pub use native_reply::{LocalApiAccepted, NativeAccepted};
pub(crate) use start::StartError;
pub use wake::{OwnerWake, OwnerWakeSlot};

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
    health: OwnerHealth,
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
        let health = OwnerHealth::new();
        let owner_health = health.clone();
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
                let mut service = match Self::open_for_actor(
                    &directory,
                    &supervisor,
                    &lost,
                    lifetime,
                    owner_notifier(&wake),
                ) {
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
                service.owner_health = owner_health.clone();
                let mut active = false;
                let mut remote_install: Option<std::sync::Arc<approvals::RemoteInstall>> = None;
                let mut controllers = HashMap::new();
                let mut private = private::PrivateOwner::new();
                // Tasks that arrived while a close was in flight, in arrival order.
                let mut deferred = std::collections::VecDeque::new();
                // Attaches waiting for the terminal they started. Other requests
                // keep running meanwhile; each attach runs once its start settled.
                let mut starting: Vec<(shellfox_core::TabId, Task)> = Vec::new();
                dispatcher.bind_wake(wake.clone());
                let ended = owner_health.end_guard();
                loop {
                    if lost.is_cancelled() || owner_abandon.is_cancelled() {
                        break;
                    }
                    owner_health.pass_started();
                    wake.rearm();
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        owner_health.step(Step::Power);
                        service.pump_power();
                        service.pump();
                    }))
                    .is_err()
                    {
                        // Retain the Service and original blocking-worker handles
                        // through the same cleanup path as dispatcher failures.
                        break;
                    }
                    owner_health.step(Step::Private);
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        private.pump(&mut service)
                    }))
                    .is_err()
                    {
                        break;
                    }
                    owner_health.step(Step::Dispatch);
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
                    owner_health.step(Step::Launches);
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        service.finish_launches()
                    }))
                    .is_err()
                    {
                        break;
                    }
                    let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        starting
                            .iter()
                            .position(|(tab, _)| service.start_settled(*tab))
                    }));
                    let Ok(settled) = settled else {
                        break;
                    };
                    let next = match settled {
                        Some(index) => Some(starting.remove(index).1),
                        None if !service.metadata_busy() => deferred.pop_front(),
                        None => None,
                    };
                    let received = match next {
                        Some(task) => Ok(task),
                        None => {
                            if deferred.len() >= MAX_DEFERRED {
                                // Leave the rest in the bounded inbox, so senders see "busy".
                                // Requests that could flow wait there too, at most until
                                // the close's deadline.
                                service.pause_for_runtime(service.owner_pass_timeout());
                                continue;
                            }
                            let timeout = service.owner_pass_timeout();
                            owner_health.waiting();
                            let received = incoming.recv_timeout(timeout);
                            owner_health.pass_started();
                            received
                        }
                    };
                    owner_health.step(Step::Task);
                    let received = match received {
                        Ok(task) if service.metadata_busy() && !task.flows(&service) => {
                            deferred.push_back(task);
                            continue;
                        }
                        // A client's detach also ends its attaches still waiting
                        // for a start: they would attach a client that left.
                        Ok(task) if task.detaches().is_some() => {
                            let client = task.detaches();
                            let (ended, kept) = std::mem::take(&mut starting)
                                .into_iter()
                                .partition(|(_, waiting): &(_, Task)| waiting.client() == client);
                            starting = kept;
                            for (_, waiting) in ended {
                                waiting.fail(anyhow::anyhow!(
                                    "The client detached before its attach finished."
                                ));
                            }
                            Ok(task)
                        }
                        Ok(task) if starting.len() < MAX_DEFERRED => {
                            match task.start_target(&private) {
                                Some(tab) => match std::panic::catch_unwind(
                                    std::panic::AssertUnwindSafe(|| service.begin_start(tab)),
                                ) {
                                    Ok(Ok(true)) => {
                                        starting.push((tab, task));
                                        continue;
                                    }
                                    Ok(Ok(false)) => Ok(task),
                                    Ok(Err(error)) => {
                                        task.fail(error);
                                        continue;
                                    }
                                    Err(_) => break,
                                },
                                None => Ok(task),
                            }
                        }
                        received => received,
                    };
                    match received {
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
                            if let Some(target) = request.close_target() {
                                let admitted = shellfox_protocol::Validate::validate(&request)
                                    .map_err(anyhow::Error::from);
                                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    service.close_request(
                                        target,
                                        admitted,
                                        Box::new(move |result| {
                                            let _ =
                                                reply.send(result.map(LocalApiResponse::Snapshot));
                                        }),
                                    )
                                }))
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                            let detach = matches!(request, LocalApiRequest::Detach {});
                            // Complete metadata mutations; only unresolved ownership
                            // offers follow recipient cancellation into exact cleanup.
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                service.execute_for_recipient(request, controller, cancel)
                            })) {
                                Ok(result) => {
                                    if detach && result.is_ok() {
                                        controllers.remove(&client_id);
                                    }
                                    service.reply_after_launch(
                                        result,
                                        Box::new(move |result| drop(reply.send(result))),
                                    );
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
                                service.request_power(
                                    request,
                                    cancel,
                                    crate::power::service::PowerReply::Native(reply),
                                );
                                continue;
                            }
                            if matches!(*action, crate::native::NativeAction::TabStats) {
                                service
                                    .enqueue_tab_stats(crate::tab_stats::StatsReply::Native(reply));
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
                            if let Some(target) = action.close_target() {
                                let admitted = action.validate().and_then(|()| {
                                    anyhow::ensure!(
                                        controller != 0,
                                        "A native controller identity is required."
                                    );
                                    Ok(())
                                });
                                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    service.close_request(
                                        target,
                                        admitted,
                                        Box::new(move |result| {
                                            let _ = reply.send(
                                                result.map(crate::native::NativeResponse::Snapshot),
                                            );
                                        }),
                                    )
                                }))
                                .is_err()
                                {
                                    break;
                                }
                                continue;
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
                if !lost.is_cancelled() && !owner_abandon.is_cancelled() {
                    eprintln!("Shellfox host: the service owner stopped; the host exits.");
                }
                drop(ended);
                let _ = finished.send(cleanup::finish(service));
            })?;
        Ok(Self {
            sender,
            started: Some(started),
            done,
            thread: Some(thread),
            abandon,
            health,
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
    pub(crate) fn health(&self) -> OwnerHealth {
        self.health.clone()
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
