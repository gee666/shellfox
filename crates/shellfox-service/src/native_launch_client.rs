//! In-process native launch capabilities, deliberately absent from wire DTOs.
use super::NativeClient;
use crate::desktop_launch::*;
impl NativeClient {
    pub async fn desktop_register(&self) -> anyhow::Result<DesktopRegistration> {
        self.owner.desktop_register().await
    }
    pub async fn desktop_submit(
        &self,
        owner: LaunchOwnerFence,
        intent: DesktopLaunchIntent,
        submission: LaunchRequestKind,
    ) -> anyhow::Result<LaunchReceipt> {
        self.owner.desktop_submit(owner, intent, submission).await
    }
    pub async fn desktop_unregister(&self, token: DesktopClientToken) -> anyhow::Result<()> {
        self.owner.desktop_unregister(token).await
    }
    pub async fn desktop_claim(
        &self,
        delivery: DesktopLaunchDelivery,
    ) -> anyhow::Result<LaunchClaim> {
        self.owner.desktop_claim(delivery).await
    }
    pub async fn desktop_complete(
        &self,
        delivery: DesktopLaunchDelivery,
        handling: LaunchHandling,
    ) -> anyhow::Result<LaunchReceipt> {
        self.owner.desktop_complete(delivery, handling).await
    }
}
