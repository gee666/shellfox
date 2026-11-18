//! Existing portable inventory does not contain a validated per-Host CLI/Desktop
//! launch context. Keep mutation availability truthful instead of inventing one.
#[path = "native_provider_portable_inventory.rs"]
mod inventory;
use crate::system::{
    self, CliInstallPlan, DesktopLaunch, SystemInstallResources, SystemResult, SystemScope,
};
use anyhow::Result;
use inventory::Inventory;
use std::{path::Path, sync::Arc};

struct PortableResources {
    inventory: Arc<Inventory>,
}
fn no_launch_context<T>() -> SystemResult<T> {
    Err(shellfox_protocol::AppError {
        code: shellfox_protocol::ErrorCode::Unsupported,
        message: "Portable inventory is verified, but an explicit validated Host data/helper launch context is required for CLI or file-manager installation.".into(),
        retryable: false,
    })
}
impl SystemInstallResources for PortableResources {
    fn validate_current(&self) -> SystemResult<()> {
        self.inventory
            .validate_current()
            .map_err(|error| shellfox_protocol::AppError {
                code: shellfox_protocol::ErrorCode::NativeUnavailable,
                message: format!("Configured portable resources unavailable: {error}"),
                retryable: false,
            })?;
        system::validate_stable_package_root(&self.inventory.root)
    }
    fn cli_plan(&self, _: SystemScope) -> SystemResult<CliInstallPlan> {
        self.validate_current()?;
        no_launch_context()
    }
    fn desktop_launch(&self) -> SystemResult<DesktopLaunch> {
        self.validate_current()?;
        no_launch_context()
    }
}
/// EXTERNAL trusted build digest only. Corrupt, unavailable and temporary layouts
/// fail closed. A genuine inventory alone never advertises install capability.
pub fn portable_install_resources(
    path: &Path,
    expected_digest: [u8; 32],
) -> Result<Arc<dyn SystemInstallResources>> {
    let inventory = inventory::load_inventory(path, expected_digest)?;
    system::validate_stable_package_root(&inventory.root)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    Ok(Arc::new(PortableResources { inventory }))
}
