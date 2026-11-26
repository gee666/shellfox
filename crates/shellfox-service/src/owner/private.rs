//! Private wire actions and non-serializable trusted native launch capabilities.
use super::*;
use crate::{desktop_launch::*, local::PrivateRequest};
use serde_json::Value;
mod directory;
mod operator;
mod outgoing;
use directory::ExistingLocalDirectory;
mod attached_launch;
mod requests;

pub(super) enum PrivateTask {
    RemoteOperator(Box<operator::Submission>),
    InstallSystem {
        backend: std::sync::Arc<crate::system::NativeSystemBackend>,
        reply: oneshot::Sender<Result<()>>,
    },
    InstallSshProfiles {
        store: crate::ssh::Store,
        reply: oneshot::Sender<Result<()>>,
    },
    OpenLifetime {
        client_id: uuid::Uuid,
        epoch: uuid::Uuid,
        reply: oneshot::Sender<Result<uuid::Uuid>>,
    },
    CloseLifetime {
        client_id: uuid::Uuid,
        epoch: uuid::Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
    CloseLifetimes {
        reply: oneshot::Sender<Result<()>>,
    },
    OutgoingFence {
        client_id: uuid::Uuid,
        epoch: uuid::Uuid,
        reply: oneshot::Sender<Result<shellfox_protocol::CancellationToken>>,
    },
    Request {
        cancel: shellfox_protocol::CancellationToken,
        request: PrivateRequest,
        client_id: uuid::Uuid,
        epoch: Option<uuid::Uuid>,
        reply: oneshot::Sender<Result<Value>>,
    },
    Register {
        reply: oneshot::Sender<Result<DesktopRegistration>>,
    },
    Submit {
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
        reply: oneshot::Sender<Result<LaunchReceipt>>,
    },
    Unregister {
        token: DesktopClientToken,
        reply: oneshot::Sender<Result<()>>,
    },
    Claim {
        delivery: DesktopLaunchDelivery,
        reply: oneshot::Sender<Result<LaunchClaim>>,
    },
    Complete {
        delivery: DesktopLaunchDelivery,
        handling: LaunchHandling,
        reply: oneshot::Sender<Result<LaunchReceipt>>,
    },
}
struct PrivateLifetime {
    epoch: uuid::Uuid,
    cancel: shellfox_protocol::CancellationToken,
}
pub(super) struct PrivateOwner {
    launches: DesktopLaunchRouter,
    attached_desktop: Option<attached_launch::AttachedRegistration>,
    lifetimes: HashMap<uuid::Uuid, PrivateLifetime>,
}
impl PrivateOwner {
    pub fn new() -> Self {
        Self {
            launches: DesktopLaunchRouter::new(),
            attached_desktop: None,
            lifetimes: HashMap::new(),
        }
    }
    /// Runs every owner pass, after any task, so receipt changes from tasks are published here too.
    pub fn pump(&mut self, service: &mut Service) {
        // A launch creates metadata, which would invalidate a close in flight.
        if service.metadata_busy() && !service.stopping {
            service.publish_launch_receipts(self.launches.generation());
            return;
        }
        if service.stopping || service.ownership_lost.is_cancelled() {
            self.cancel_outgoing_fences();
            self.launches.invalidate_owner();
        } else {
            let owner = self.launches.owner();
            let mut resolver = ServiceLaunchResolver::new(service, &ExistingLocalDirectory);
            // Full/no registered desktop retains queued intent without creating any
            // metadata. The next owner tick retries delivery, never the UI effect.
            let _ = self.launches.dispatch_one(owner, &mut resolver);
        }
        service.publish_launch_receipts(self.launches.generation());
    }
    pub fn handle(
        &mut self,
        service: &mut Service,
        task: PrivateTask,
        controllers: &mut HashMap<uuid::Uuid, u64>,
    ) {
        match task {
            PrivateTask::RemoteOperator(task) => self.submit_remote_operator(service, *task),
            PrivateTask::InstallSystem { backend, reply } => {
                let _ = reply.send(
                    service
                        .install_system_native(backend)
                        .map_err(|error| anyhow::anyhow!(error.message)),
                );
            }
            PrivateTask::InstallSshProfiles { store, reply } => {
                let result = service.assert_accepting().and_then(|()| {
                    anyhow::ensure!(
                        service.ssh_profiles.is_none(),
                        "SSH profiles are already installed."
                    );
                    service.ssh_profiles = Some(std::sync::Arc::new(store));
                    Ok(())
                });
                let _ = reply.send(result);
            }
            PrivateTask::OpenLifetime {
                client_id,
                epoch,
                reply,
            } => {
                // A failed metadata flush may leave a stopping owner available
                // for authenticated final reads/Stop retry. Program admission is
                // fenced separately at the actual mutation handlers.
                let result = service.assert_owned().and_then(|()| {
                    if client_id.is_nil()
                        || epoch.is_nil()
                        || (controllers.contains_key(&client_id)
                            && !self.lifetimes.contains_key(&client_id))
                    {
                        bail!("Private client identity is unavailable.");
                    }
                    if self.lifetimes.len() >= 128 && !self.lifetimes.contains_key(&client_id) {
                        bail!("Private lifetime limit reached.");
                    }
                    if let Some(controller) = controllers.remove(&client_id) {
                        service.detach_controller(controller);
                    }
                    self.forget_attached_desktop(service, client_id);
                    if let Some(previous) = self.lifetimes.insert(
                        client_id,
                        PrivateLifetime {
                            epoch,
                            cancel: shellfox_protocol::CancellationToken::new(),
                        },
                    ) {
                        previous.cancel.cancel();
                    }
                    Ok(epoch)
                });
                let _ = reply.send(result);
            }
            PrivateTask::CloseLifetimes { reply } => {
                self.attached_desktop = None;
                for (id, lifetime) in self.lifetimes.drain() {
                    lifetime.cancel.cancel();
                    if let Some(controller) = controllers.remove(&id) {
                        service.detach_controller(controller);
                    }
                }
                let _ = reply.send(Ok(()));
            }
            PrivateTask::CloseLifetime {
                client_id,
                epoch,
                reply,
            } => {
                if self.lifetimes.get(&client_id).map(|l| l.epoch) == Some(epoch) {
                    self.forget_attached_desktop(service, client_id);
                    if let Some(lifetime) = self.lifetimes.remove(&client_id) {
                        lifetime.cancel.cancel();
                    }
                    if let Some(controller) = controllers.remove(&client_id) {
                        service.detach_controller(controller);
                    }
                }
                let _ = reply.send(Ok(()));
            }
            PrivateTask::OutgoingFence {
                client_id,
                epoch,
                reply,
            } => {
                let _ = reply.send(self.caller_fence(service, client_id, epoch));
            }
            PrivateTask::Request {
                cancel,
                request,
                client_id,
                epoch,
                reply,
            } => {
                self.handle_request(
                    service,
                    controllers,
                    request,
                    requests::RequestContext {
                        client_id,
                        epoch,
                        cancel,
                    },
                    reply,
                );
            }
            PrivateTask::Submit {
                owner,
                intent,
                submission,
                reply,
            } => {
                let result = service.assert_accepting().and_then(|()| {
                    if !self.launches.route_info(service)?.desktop_registered {
                        return Err(LaunchError::NoDesktop.into());
                    }
                    match submission {
                        LaunchRequestKind::Fresh => self.launches.enqueue(owner, intent, service),
                        LaunchRequestKind::Known => self.launches.resume(owner, intent, service),
                    }
                    .map_err(anyhow::Error::from)
                });
                let _ = reply.send(result);
            }
            PrivateTask::Register { reply } => {
                let result = service.assert_accepting().and_then(|()| {
                    self.launches
                        .register(self.launches.owner(), service)
                        .map_err(anyhow::Error::from)
                });
                let _ = reply.send(result);
            }
            PrivateTask::Unregister { token, reply } => {
                let result = self
                    .launches
                    .unregister(token, service)
                    .map_err(anyhow::Error::from);
                let _ = reply.send(result);
            }
            PrivateTask::Claim { delivery, reply } => {
                let result = service.assert_accepting().and_then(|()| {
                    self.launches
                        .claim(
                            &delivery,
                            &mut ServiceLaunchResolver::new(service, &ExistingLocalDirectory),
                        )
                        .map_err(anyhow::Error::from)
                });
                let _ = reply.send(result);
            }
            PrivateTask::Complete {
                delivery,
                handling,
                reply,
            } => {
                let result = self
                    .launches
                    .complete(&delivery, handling, service)
                    .map_err(anyhow::Error::from);
                let _ = reply.send(result);
            }
        }
    }
}
fn encode(value: impl serde::Serialize) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

impl PrivateTask {
    pub(super) fn client(&self) -> Option<uuid::Uuid> {
        match self {
            Self::Request { client_id, .. } => Some(*client_id),
            _ => None,
        }
    }
    pub(super) fn detaches(&self) -> bool {
        match self {
            Self::Request {
                request: PrivateRequest::LocalApi(LocalApiRequest::Detach {}),
                ..
            } => true,
            Self::Request {
                request: PrivateRequest::DesktopView(view),
                ..
            } => {
                matches!(view.as_ref(), crate::local::DesktopRequest::DetachAll)
            }
            _ => false,
        }
    }
    pub(super) fn fail(self, error: anyhow::Error) {
        if let Self::Request { reply, .. } = self {
            let _ = reply.send(Err(error));
        }
    }
    /// Whether this task may run while a close is in flight (see close.rs).
    pub(super) fn flows(&self, service: &Service) -> bool {
        let Self::Request { request, .. } = self else {
            return false;
        };
        match request {
            PrivateRequest::LocalApi(request) => service.local_api_flows(request),
            PrivateRequest::DesktopView(request) => {
                crate::native::NativeAction::try_from((**request).clone())
                    .is_ok_and(|action| service.native_flows(&action))
            }
            PrivateRequest::Power(_) => true,
            _ => false,
        }
    }
}

impl PrivateTask {
    /// The tab an attach would start; see `Task::start_target`. None for a
    /// stale lifetime epoch, which the request itself then refuses.
    pub(super) fn start_target(&self, owner: &PrivateOwner) -> Option<shellfox_core::TabId> {
        use shellfox_protocol::Validate;
        let Self::Request {
            request,
            client_id,
            epoch,
            ..
        } = self
        else {
            return None;
        };
        if epoch.is_some_and(|epoch| owner.lifetimes.get(client_id).map(|l| l.epoch) != Some(epoch))
        {
            return None;
        }
        match request {
            PrivateRequest::LocalApi(request @ LocalApiRequest::Attach { tab, .. }) => {
                request.validate().is_ok().then_some(*tab)
            }
            PrivateRequest::DesktopView(view) => match view.as_ref() {
                crate::local::DesktopRequest::Attach { tab, .. } => {
                    view.validate().is_ok().then_some(*tab)
                }
                _ => None,
            },
            _ => None,
        }
    }
}
