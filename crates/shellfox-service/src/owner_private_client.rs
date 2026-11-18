use super::*;
use crate::{desktop_launch::*, local::PrivateRequest};
use private::PrivateTask;

impl OwnerClient {
    pub(super) fn submit_private(&self, task: PrivateTask) -> Result<()> {
        match self.sender.try_send(Task::Private(Box::new(task))) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                bail!("Private owner request was not admitted: queue full.")
            }
            Err(TrySendError::Disconnected(_)) => bail!("Private owner stopped before admission."),
        }
    }
    /// Explicit owner-native policy, never a wire request or privilege fact.
    pub async fn install_observer_factory_policy(
        &self,
        policy: crate::NativeFactoryPolicy,
    ) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::InstallFactoryPolicy { policy, reply })?;
        response.await.context(
            "Native factory policy installation reply lost; source stays fenced on uncertainty.",
        )?
    }
    /// Protected native bootstrap only. Backend resources are externally validated;
    /// the real Service selects its final ownership-loss token internally.
    pub async fn install_system_native(
        &self,
        backend: std::sync::Arc<crate::system::NativeSystemBackend>,
    ) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::InstallSystem { backend, reply })?;
        response
            .await
            .context("Native system installation reply lost; no automatic retry.")?
    }
    /// Trusted bootstrap resources only; never exposed by any wire DTO.
    pub async fn install_ssh_native(&self, config: crate::ssh::NativeSshInstall) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::InstallSsh { config, reply })?;
        response
            .await
            .context("SSH bootstrap installation reply lost; no automatic retry.")?
    }
    /// Explicit native security reduction, not view detach or cleanup proof.
    pub async fn revoke_native_ssh(&self) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::RevokeSsh {
            client_id: self.client_id,
            reply,
        })?;
        response
            .await
            .context("Native SSH revocation reply lost.")?
    }
    pub(crate) fn for_private_client(&self, client_id: uuid::Uuid) -> Self {
        Self {
            sender: self.sender.clone(),
            client_id,
            remote: self.remote.clone(),
        }
    }
    pub(crate) async fn open_lifetime(&self, epoch: uuid::Uuid) -> Result<uuid::Uuid> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::OpenLifetime {
            client_id: self.client_id,
            epoch,
            reply,
        })?;
        response
            .await
            .context("Private lifetime opening reply lost.")?
    }
    pub(crate) async fn close_all_lifetimes(&self) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private_cleanup(PrivateTask::CloseLifetimes { reply })
            .await?;
        response
            .await
            .context("Private lifetime drain reply lost.")?
    }
    pub(crate) async fn close_lifetime(&self, epoch: uuid::Uuid) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private_cleanup(PrivateTask::CloseLifetime {
            client_id: self.client_id,
            epoch,
            reply,
        })
        .await?;
        response
            .await
            .context("Private lifetime release reply lost.")?
    }
    async fn submit_private_cleanup(&self, task: PrivateTask) -> Result<()> {
        let mut task = Task::Private(Box::new(task));
        loop {
            match self.sender.try_send(task) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) => {
                    task = returned;
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                Err(TrySendError::Disconnected(_)) => {
                    bail!("Owner stopped before lifetime release admission.")
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) async fn private_request(
        &self,
        request: PrivateRequest,
    ) -> Result<serde_json::Value> {
        self.request_with_epoch(request, None).await
    }
    pub(crate) async fn private_request_in_epoch(
        &self,
        request: PrivateRequest,
        epoch: uuid::Uuid,
    ) -> Result<serde_json::Value> {
        self.request_with_epoch(request, Some(epoch)).await
    }
    async fn request_with_epoch(
        &self,
        request: PrivateRequest,
        epoch: Option<uuid::Uuid>,
    ) -> Result<serde_json::Value> {
        if let PrivateRequest::OutgoingV2(request) = request {
            return self
                .private_outgoing(
                    request,
                    epoch.context("Outgoing requires the original private lifetime epoch")?,
                )
                .await;
        }
        if let PrivateRequest::RemoteOperator(request) = request {
            return self
                .private_remote_operator(
                    request,
                    epoch.context("Remote operator requires the original lifetime epoch")?,
                )
                .await;
        }
        let (reply, response) = oneshot::channel();
        let recipient = crate::grant_requests::RecipientCancellation::new();
        self.submit_private(PrivateTask::Request {
            cancel: recipient.token(),
            request,
            client_id: self.client_id,
            epoch,
            reply,
        })?;
        response.await.context(
            "Private owner reply lost; mutation outcome is uncertain and was not retried.",
        )?
    }
    /// Trusted in-process native API only. No corresponding wire DTO exists.
    pub async fn desktop_register(&self) -> Result<DesktopRegistration> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::Register { reply })?;
        response
            .await
            .context("Desktop registration reply lost; no automatic retry.")?
    }
    /// In-process bootstrap admission only; returns Queued, never waits for UI.
    pub async fn desktop_submit(
        &self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
    ) -> Result<LaunchReceipt> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::Submit {
            owner,
            intent,
            submission,
            reply,
        })?;
        response
            .await
            .context("Desktop submit reply lost; intent must not be replayed.")?
    }
    pub async fn desktop_unregister(&self, token: DesktopClientToken) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::Unregister { token, reply })?;
        response.await.context("Desktop unregister reply lost.")?
    }
    pub async fn desktop_claim(&self, delivery: DesktopLaunchDelivery) -> Result<LaunchClaim> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::Claim { delivery, reply })?;
        response
            .await
            .context("Desktop claim reply lost; UI effects must not be applied.")?
    }
    pub async fn desktop_complete(
        &self,
        delivery: DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> Result<LaunchReceipt> {
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::Complete {
            delivery,
            handling,
            reply,
        })?;
        response
            .await
            .context("Desktop completion reply lost; UI effects must not be replayed.")?
    }
}
