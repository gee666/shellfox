//! GPUI ownership only. The host contracts do not depend on GPUI.
use super::AppHost;
use crate::host_client::HostEndpointConfig;
use anyhow::{Result, ensure};
use gpui::{App, Global};

pub struct AppHostGlobal {
    config: HostEndpointConfig,
    host: AppHost,
}
impl Global for AppHostGlobal {}

/// Install the exact prestarted owner once. Borrowing keeps the caller's cleanup
/// authority intact if a mismatched Global rejects installation.
pub fn install_prestarted(
    cx: &mut App,
    config: HostEndpointConfig,
    host: &AppHost,
) -> Result<AppHost> {
    if cx.has_global::<AppHostGlobal>() {
        let existing = cx.global::<AppHostGlobal>();
        ensure!(
            existing.config.data_dir == config.data_dir
                && existing.config.helper == config.helper
                && existing.config.helper_sha256 == config.helper_sha256
                && existing.host.same_owner(host),
            "The application already retains another endpoint owner."
        );
        return Ok(existing.host.clone());
    }
    cx.set_global(AppHostGlobal {
        config,
        host: host.clone(),
    });
    Ok(host.clone())
}

pub fn get(cx: &App) -> Option<AppHost> {
    cx.has_global::<AppHostGlobal>()
        .then(|| cx.global::<AppHostGlobal>().host.clone())
}
