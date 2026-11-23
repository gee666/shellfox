//! The desktop window's side of launch routing: register, then claim and
//! acknowledge each launch the host hands to this UI.
use super::HostClient;
use shellfox_service::desktop_launch::*;

impl HostClient {
    pub async fn desktop_register(&self) -> anyhow::Result<DesktopRegistration> {
        self.port.register_desktop().await
    }
    pub async fn desktop_submit(
        &self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
    ) -> anyhow::Result<LaunchReceipt> {
        let DesktopLaunchReply::Receipt(receipt) = self
            .port
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
    }
    pub async fn desktop_unregister(&self, token: DesktopClientToken) -> anyhow::Result<()> {
        let DesktopLaunchReply::Unregistered {} = self
            .port
            .request(DesktopLaunchRequest::UnregisterAttached { token })
            .await?
        else {
            anyhow::bail!("Invalid attached desktop unregister response.");
        };
        Ok(())
    }
    pub async fn desktop_claim(
        &self,
        delivery: DesktopLaunchDelivery,
    ) -> anyhow::Result<LaunchClaim> {
        let DesktopLaunchReply::Claim(claim) = self
            .port
            .request(DesktopLaunchRequest::ClaimAttached { delivery })
            .await?
        else {
            anyhow::bail!("Invalid attached desktop launch claim.");
        };
        Ok(claim)
    }
    pub async fn desktop_complete(
        &self,
        delivery: DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> anyhow::Result<LaunchReceipt> {
        let request_id = delivery.intent.request_id;
        let DesktopLaunchReply::Receipt(receipt) = self
            .port
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
    }
}
