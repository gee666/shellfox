//! Launch-registration metadata retires with the host it was registered on.
use crate::{app_host::AppHost, host_client::HostClient};
use shellfox_service::desktop_launch::DesktopClientToken;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unregistered {
    Receipt,
    RetiredWithOriginalHostStop,
}
pub(super) async fn finish(
    client: &HostClient,
    token: DesktopClientToken,
    original: &AppHost,
) -> anyhow::Result<Unregistered> {
    // Serialize with an in-flight last-window Stop, not with arbitrary error text.
    if original.stopped_after_operations().await.unwrap_or(false) {
        return Ok(Unregistered::RetiredWithOriginalHostStop);
    }
    match client.desktop_unregister(token).await {
        Ok(()) => Ok(Unregistered::Receipt),
        // Stop may win after the first observation but before the host's admission.
        Err(_) if original.stopped_after_operations().await.unwrap_or(false) => {
            Ok(Unregistered::RetiredWithOriginalHostStop)
        }
        Err(error) => Err(error),
    }
}
