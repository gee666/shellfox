//! GPUI ownership only. The endpoint/service contracts do not depend on GPUI.
use super::AppHost;
use crate::host_client::HostEndpointConfig;
use anyhow::{Result, ensure};
use gpui::{App, Global};

pub struct AppHostGlobal {
    config: HostEndpointConfig,
    host: AppHost,
}
impl Global for AppHostGlobal {}

pub fn install(cx: &mut App, config: HostEndpointConfig) -> Result<AppHost> {
    install_with(cx, config, AppHost::start)
}
/// The Root's ServiceNative/OwnerDispatcher factory belongs here, once, not
/// inside every Root::open. Existing Globals never invoke the factory again.
pub fn install_with(
    cx: &mut App,
    config: HostEndpointConfig,
    start: impl FnOnce(HostEndpointConfig) -> Result<AppHost>,
) -> Result<AppHost> {
    if cx.has_global::<AppHostGlobal>() {
        let existing = cx.global::<AppHostGlobal>();
        ensure!(
            existing.config.data_dir == config.data_dir
                && existing.config.helper == config.helper
                && existing.config.helper_sha256 == config.helper_sha256,
            "The application already owns another endpoint configuration."
        );
        return Ok(existing.host.clone());
    }
    let host = start(config.clone())?;
    cx.set_global(AppHostGlobal {
        config,
        host: host.clone(),
    });
    Ok(host)
}
/// Install the exact prestarted owner once. Borrowing keeps the caller's cleanup
/// authority intact if a mismatched Global rejects installation.
pub fn install_prestarted(
    cx: &mut App,
    config: HostEndpointConfig,
    host: &AppHost,
) -> Result<AppHost> {
    if cx.has_global::<AppHostGlobal>() {
        let existing = cx.global::<AppHostGlobal>();
        validate_prestarted(&existing.config, &existing.host, &config, host)?;
        return Ok(existing.host.clone());
    }
    cx.set_global(AppHostGlobal {
        config,
        host: host.clone(),
    });
    Ok(host.clone())
}

pub(super) fn validate_prestarted<B: super::AppHostBackend>(
    existing: &HostEndpointConfig,
    owner: &super::AppHost<B>,
    config: &HostEndpointConfig,
    candidate: &super::AppHost<B>,
) -> Result<()> {
    ensure!(
        existing.data_dir == config.data_dir
            && existing.helper == config.helper
            && existing.helper_sha256 == config.helper_sha256
            && owner.same_owner(candidate),
        "The application already retains another endpoint owner."
    );
    Ok(())
}

pub fn get(cx: &App) -> Option<AppHost> {
    cx.has_global::<AppHostGlobal>()
        .then(|| cx.global::<AppHostGlobal>().host.clone())
}
