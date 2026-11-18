//! Readonly owner-bound launch catalog. The GUI does not probe its filesystem.
use super::{HostClient, NativeAction, NativeResponse};
impl HostClient {
    pub async fn terminal_profiles(&self) -> anyhow::Result<shellfox_service::NativeLaunchCatalog> {
        match self.execute(NativeAction::TerminalProfiles).await? {
            NativeResponse::TerminalProfiles(catalog) => Ok(*catalog),
            _ => anyhow::bail!("Owner returned a mismatched native launch catalog response."),
        }
    }
}
