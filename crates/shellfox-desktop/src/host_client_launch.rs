//! Route desktop launch intents through the same owner, whether it lives in
//! this process or the verified local headless host.
use super::{HostClient, HostEndpointConfig};
use shellfox_service::desktop_launch::*;

/// A fenced secondary launch must not replay a queued request to a new owner.
pub async fn handoff_existing(
    config: &HostEndpointConfig,
    intent: DesktopLaunchIntent,
    submission: LaunchRequestKind,
    owner: LaunchOwnerFence,
) -> anyhow::Result<LaunchReceipt> {
    let client = shellfox_service::local::LocalClient::connect(
        &config.data_dir,
        &config.helper,
        config.helper_sha256,
    )
    .await?;
    client
        .launch_existing_pinned(intent, submission, owner)
        .await
}

impl HostClient {
    pub async fn desktop_register(&self) -> anyhow::Result<DesktopRegistration> {
        if let Some(local) = self.local() {
            local.register_desktop().await
        } else {
            self.native()?.desktop_register().await
        }
    }
    pub async fn desktop_submit(
        &self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
    ) -> anyhow::Result<LaunchReceipt> {
        if let Some(local) = self.local() {
            let DesktopLaunchReply::Receipt(receipt) = local
                .request(DesktopLaunchRequest::Submit {
                    owner,
                    intent,
                    submission,
                })
                .await?
            else {
                anyhow::bail!("Invalid attached desktop launch receipt.");
            };
            Ok(receipt)
        } else {
            self.native()?
                .desktop_submit(owner, intent, submission)
                .await
        }
    }
    pub async fn desktop_unregister(&self, token: DesktopClientToken) -> anyhow::Result<()> {
        if let Some(local) = self.local() {
            let DesktopLaunchReply::Unregistered {} = local
                .request(DesktopLaunchRequest::UnregisterAttached { token })
                .await?
            else {
                anyhow::bail!("Invalid attached desktop unregister response.");
            };
            Ok(())
        } else {
            self.native()?.desktop_unregister(token).await
        }
    }
    pub async fn desktop_claim(
        &self,
        delivery: DesktopLaunchDelivery,
    ) -> anyhow::Result<LaunchClaim> {
        if let Some(local) = self.local() {
            let DesktopLaunchReply::Claim(claim) = local
                .request(DesktopLaunchRequest::ClaimAttached { delivery })
                .await?
            else {
                anyhow::bail!("Invalid attached desktop launch claim.");
            };
            Ok(claim)
        } else {
            self.native()?.desktop_claim(delivery).await
        }
    }
    pub async fn desktop_complete(
        &self,
        delivery: DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> anyhow::Result<LaunchReceipt> {
        if let Some(local) = self.local() {
            let request_id = delivery.intent.request_id;
            let DesktopLaunchReply::Receipt(receipt) = local
                .request(DesktopLaunchRequest::CompleteAttached { delivery, handling })
                .await?
            else {
                anyhow::bail!("Invalid attached desktop completion receipt.");
            };
            anyhow::ensure!(
                receipt.request_id == request_id,
                "Attached desktop completion request mismatch."
            );
            Ok(receipt)
        } else {
            self.native()?.desktop_complete(delivery, handling).await
        }
    }
}
