//! Private wire actions and non-serializable trusted native launch capabilities.
use super::*;
use crate::{desktop_launch::*, local::PrivateRequest};
use serde_json::Value;
#[path = "owner_private_directory.rs"]
mod directory;
#[path = "owner_private_operator.rs"]
mod operator;
#[path = "owner_private_outgoing.rs"]
mod outgoing;
use directory::ExistingLocalDirectory;
#[path = "owner_attached_launch.rs"]
mod attached_launch;
#[path = "owner_private_requests.rs"]
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
        epoch: Option<uuid::Uuid>,
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
    outgoing_native: shellfox_protocol::CancellationToken,
}
impl PrivateOwner {
    pub fn new() -> Self {
        Self {
            launches: DesktopLaunchRouter::new(),
            attached_desktop: None,
            lifetimes: HashMap::new(),
            outgoing_native: shellfox_protocol::CancellationToken::new(),
        }
    }
    /// Runs every owner pass, after any task, so receipt changes from tasks are published here too.
    pub fn pump(&mut self, service: &mut Service) {
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
                    anyhow::ensure!(service.ssh_profiles.is_none(), "SSH profiles are already installed.");
                    service.ssh_profiles = Some(store);
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
            PrivateTask::Request { cancel, request, client_id, epoch, reply } => {
                self.handle_request(service, controllers, request, requests::RequestContext { client_id, epoch, cancel }, reply);
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
