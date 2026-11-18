//! Native launch coordination uses the existing in-process owner, never a wire
//! registration DTO or a second Host.
use super::{HostClient, HostEndpointConfig};

/// Existing-owner mode is only an authenticated launch handoff, followed by
/// secondary-process exit. It cannot attach a renderer to a headless owner.
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
use shellfox_service::desktop_launch::*;
impl HostClient {
    pub async fn desktop_register(&self) -> anyhow::Result<DesktopRegistration> {
        self.native.desktop_register().await
    }
    pub async fn desktop_submit(&self, owner: LaunchOwnerFence, intent: DesktopLaunchIntent, submission: LaunchRequestKind) -> anyhow::Result<LaunchReceipt> { self.native.desktop_submit(owner,intent,submission).await }
    pub async fn desktop_unregister(&self, token: DesktopClientToken) -> anyhow::Result<()> {
        self.native.desktop_unregister(token).await
    }
    pub async fn desktop_claim(
        &self,
        delivery: DesktopLaunchDelivery,
    ) -> anyhow::Result<LaunchClaim> {
        self.native.desktop_claim(delivery).await
    }
    pub async fn desktop_complete(
        &self,
        delivery: DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> anyhow::Result<LaunchReceipt> {
        self.native.desktop_complete(delivery, handling).await
    }
}
