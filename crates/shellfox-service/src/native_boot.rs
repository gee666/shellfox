//! Every resource acquired after the Go lock stays here until explicit cleanup.
use super::preparation::Preparation;
use super::{LocalBootStatus, Record, NAMESPACE, TRANSPORT_VERSION};
use crate::{owner::ServiceOwner, platform, security::PrivateDirectory, OwnerDispatcher};
use anyhow::{Context, Result};
use std::time::Duration;
#[path = "native_endpoint_publication.rs"]
mod endpoint;
use endpoint::EndpointPublication;
#[path = "native_control_files.rs"]
mod control_files;
pub(crate) use control_files::NativeControlChild;
#[path = "native_boot_remote.rs"]
mod remote_boot;
#[path = "native_boot_providers.rs"]
mod provider_boot;
use control_files::NativeControlFiles;
use remote_boot::RemoteInitializationFailure;
#[cfg(test)]
#[path = "native_boot_dirty_tests.rs"]
mod dirty_tests;

pub(super) struct BootGuard {
    pub private: PrivateDirectory,
    pub(super) preparation: Preparation,
    pub(super) server: super::ServerState,
    control_files: Option<NativeControlFiles>,
    pub listener: Option<platform::Listener>,
    pub actor: Option<ServiceOwner>,
    pub remote: Option<crate::remote::RemoteHostSetup>,
    pub remote_control: Option<crate::remote::RemoteControl>,
    pub(super) published: Option<Record>,
    pending_record: Option<Record>,
    endpoint: EndpointPublication,
    shutdown: Option<crate::ShutdownOutcome>,
    install_source: Option<super::SourceFactsInstaller>,
    supervisor: Option<std::path::PathBuf>,
    observer_factory_policy: crate::NativeFactoryPolicy,
    providers: provider_boot::NativeProviderState,
}
impl BootGuard {
    pub fn new(private: PrivateDirectory) -> Result<Self> {
        let preparation = Preparation::new(&private)?;
        Ok(Self {
            private,
            preparation,
            server: super::ServerState::new(),
            control_files: None,
            listener: None,
            actor: None,
            remote: None,
            remote_control: None,
            published: None,
            pending_record: None,
            endpoint: EndpointPublication::new(),
            shutdown: None,
            install_source: None,
            supervisor: None,
            observer_factory_policy: crate::NativeFactoryPolicy::Disabled,
            providers: provider_boot::NativeProviderState::default(),
        })
    }
    fn control_files(&mut self) -> Result<&mut NativeControlFiles> {
        if self.control_files.is_none() {
            let owner = self.preparation.acquired_view()?;
            self.control_files = Some(NativeControlFiles::new(
                &owner,
                self.private.creation_owner(),
            ));
        }
        self.control_files
            .as_mut()
            .context("Native control owner missing")
    }
    pub async fn start<D: OwnerDispatcher + 'static>(&mut self, dispatcher: D) -> Result<()> {
        self.start_actor(dispatcher).await?;
        self.publish_activate().await
    }
    async fn start_actor<D: OwnerDispatcher + 'static>(&mut self, dispatcher: D) -> Result<()> {
        anyhow::ensure!(
            self.actor.is_none() && self.listener.is_none(),
            "Original local actor is already installed"
        );
        self.server.ensure_running()?;
        self.control_files()?;
        let ownership = self.preparation.acquired_view()?;
        anyhow::ensure!(ownership.is_alive(), "Native host ownership lost.");
        self.private
            .prepare_owned_layout(ownership.permission_verifier())
            .await?;
        self.server.ensure_running()?;
        self.control_files
            .as_mut()
            .context("Native control owner missing")?
            .bind(&self.private)?;
        self.listener = Some(platform::Listener::bind_verified(
            &self.private.path,
            #[cfg(unix)]
            self.private.endpoint_directory()?,
        )?);
        self.pending_record = Some(Record {
            namespace: NAMESPACE.into(),
            transport_version: TRANSPORT_VERSION,
            address: self
                .listener
                .as_ref()
                .context("Listener missing.")?
                .address(),
            secret: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        });
        let data = self
            .private
            .path
            .parent()
            .context("Private data root missing.")?;
        if let Err(error) = crate::electron_import::import_electron_root(
            data,
            &self.private.path,
            ownership.permission_verifier().clone(),
            ownership.lost(),
        ).await {
            eprintln!("Electron import did not complete: {error:#}");
        }
        if let Err(error) = ServiceOwner::start_with_root_lifetime(
            &mut self.actor,
            data.to_owned(),
            match &self.supervisor {
                Some(supervisor) => supervisor.clone(),
                // Ordinary compatibility entry runs inside its actual CLI host.
                None => std::env::current_exe()?,
            },
            ownership.lost(),
            ownership.root_lifetime(),
            dispatcher,
        )
        .await
        {
            if let Some(failure) = error.downcast_ref::<crate::owner::StartupFailure>() {
                self.shutdown = Some(failure.shutdown.clone());
            }
            return Err(error);
        }
        self.server.ensure_running()?;
        if let Some(remote) = &self.remote {
            let actor = self
                .actor
                .as_ref()
                .context("Original actor missing before repository bind")?;
            remote
                .bind_repository(
                    actor
                        .client(uuid::Uuid::new_v4())
                        .approval_repository(ownership.lost()),
                )
                .map_err(|error| anyhow::Error::new(RemoteInitializationFailure(error)))?;
        }
        Ok(())
    }
    async fn publish_activate(&mut self) -> Result<()> {
        self.install_native_providers().await?;
        let ownership = self.preparation.acquired_view()?;
        self.server.ensure_running()?;
        if self.observer_factory_policy != crate::NativeFactoryPolicy::Disabled {
            self.actor
                .as_ref()
                .context("Service owner missing before native policy installation.")?
                .client(uuid::Uuid::new_v4())
                .install_observer_factory_policy(self.observer_factory_policy)
                .await?;
        }
        self.private
            .protect_artifacts(ownership.permission_verifier())
            .await?;
        self.server.ensure_running()?;
        let control = self
            .control_files
            .as_ref()
            .context("Native control owner missing")?
            .child()?;
        let record = self
            .pending_record
            .as_ref()
            .context("Original endpoint record missing")?;
        self.endpoint
            .publish(&control, &ownership, &serde_json::to_vec(record)?)?;
        self.published = self.pending_record.take();
        let actor = self.actor.as_ref().context("Service owner missing.")?;
        actor.activate().await?;
        self.server.ensure_running()?;
        if let Some(install) = &self.install_source {
            let watch = actor
                .client(uuid::Uuid::new_v4())
                .source_facts_watch()
                .await?;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| install(watch)))
                .map_err(|_| anyhow::anyhow!("Source facts installer failed."))??;
        }
        anyhow::ensure!(
            ownership.is_alive(),
            "Native ownership lost before endpoint activation"
        );
        let listener = self
            .listener
            .as_ref()
            .context("Listener closed before endpoint activation")?;
        anyhow::ensure!(
            self.published
                .as_ref()
                .is_some_and(|record| record.address == listener.address()),
            "Endpoint address differs from retained listener"
        );
        Ok(())
    }
    /// Samples actual owner health at handoff only. Consumers must preserve
    /// their current-ownership read checks; the mutation fence is not that proof.
    pub fn native_control_child(&self) -> Result<crate::local::NativeControlChild> {
        let owner = self.preparation.acquired_view()?;
        anyhow::ensure!(owner.is_alive(), "Native owner lost at control handoff");
        self.control_files
            .as_ref()
            .context("Native control owner missing")?
            .child()
    }
    pub fn endpoint_report(&self) -> std::sync::Arc<crate::NativeEndpointReport> {
        self.endpoint.report()
    }
    async fn cleanup_once(&mut self) -> Result<()> {
        self.server.request_stop();
        self.drain_native_provider_worker()?;
        let alive = self
            .preparation
            .acquired_view()
            .is_ok_and(|owner| owner.is_alive());
        let client = self
            .actor
            .as_ref()
            .map(|actor| actor.client(uuid::Uuid::new_v4()));
        self.server.drain(client, alive).await;
        if let Some(control) = &self.control_files {
            control.revoke()?;
        }
        if let Some(actor) = &mut self.actor {
            // This borrowed method retains the actor on failed confirmation.
            self.shutdown = Some(actor.shutdown().await?);
            self.actor = None;
        }
        if let Some(remote) = &mut self.remote {
            remote
                .shutdown()
                .await
                .map_err(|error| anyhow::anyhow!("Remote cleanup retained: {}", error.message))?;
            self.remote = None;
            self.remote_control = None;
            self.install_source = None;
        }
        self.endpoint.cleanup();
        self.published = None;
        self.pending_record = None;
        if let Some(listener) = self.listener.take() {
            #[cfg(unix)]
            let address = listener.address();
            drop(listener); // closes only; no ignored pathname unlink
            #[cfg(unix)]
            self.endpoint.preserve_socket(address);
        }
        // Unpin created directories; the directories themselves stay.
        self.private.creation_owner().release();
        // SAME pre-effect scope, independent of acquisition phase or facade moves.
        let _proof = self.preparation.cleanup().await?;
        Ok(())
    }
    pub async fn cleanup(&mut self, status: &impl Fn(LocalBootStatus)) -> crate::ShutdownOutcome {
        status(LocalBootStatus::RetainedCleanup);
        loop {
            match self.cleanup_once().await {
                Ok(()) => {
                    let outcome = self.shutdown.clone().unwrap_or(crate::ShutdownOutcome {
                        activity: crate::ActivityShutdownState::Committed,
                        presentation: crate::PresentationShutdownState::NoKnownFailure,
                    });
                    // Dirty-after-loss is a metadata warning, not a live-resource
                    // failure. All original resource controllers closed above.
                    crate::owner::report_warning(&outcome);
                    status(LocalBootStatus::NoOwnerClean);
                    return outcome;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}
