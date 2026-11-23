//! Stable, explicit frontend device namespace. Construct before GPUI; views receive only an Arc.
use crate::{options::Options, settings::DeviceColors};
use anyhow::{Context, Result, ensure};
use gpui::{App, Global};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct DevicePrefs(pub Arc<DeviceColors>);
impl Global for DevicePrefs {}
fn normalized(path: &Path) -> Result<PathBuf> {
    let path = path
        .to_str()
        .context("Device configuration paths must be UTF-8")?;
    Ok(crate::app_launch::path::normalize_launch_path(path)?.into())
}
// Early lexical rejection only; FileService::check_private_root refuses links and aliases.
fn comparable(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}
fn overlap(left: &Path, right: &Path) -> bool {
    let left = comparable(left);
    let right = comparable(right);
    left.starts_with(&right) || right.starts_with(&left)
}
pub(crate) fn configured_root(
    host: &Path,
    desktop: &Path,
    explicit: Option<PathBuf>,
) -> Result<PathBuf> {
    let host = normalized(host)?;
    ensure!(
        !overlap(&host, &normalized(desktop)?),
        "Host and UI scratch roots must be distinct and nonancestor"
    );
    let root = if let Some(root) = explicit {
        ensure!(
            !root
                .components()
                .any(|part| part == std::path::Component::ParentDir),
            "Device root cannot contain parent traversal"
        );
        normalized(&root)?
    } else {
        let mut name = host
            .file_name()
            .context("Host data root must have a name")?
            .to_os_string();
        name.push(".device");
        host.with_file_name(name)
    };
    ensure!(
        root.is_absolute() && root.parent().is_some() && root.file_name().is_some(),
        "Device configuration root must be an absolute non-root directory"
    );
    ensure!(
        !overlap(&root, &host) && !overlap(&root, &normalized(desktop)?),
        "Host, UI scratch and device configuration roots must be distinct and nonancestor"
    );
    Ok(root)
}
pub(crate) fn prepare(
    options: &Options,
    frontend: &mut crate::frontend_composition::FrontendDirectories,
) -> Result<Arc<DeviceColors>> {
    let proof = frontend.device_root()?;
    ensure!(
        proof.root_path() == Some(options.device_config_root.as_path()),
        "Device capability does not match configured root"
    );
    let store = DeviceColors::native_anchored(&options.device_config_root, proof)?;
    Ok(Arc::new(store))
}
pub(crate) fn get(cx: &App) -> Result<Arc<DeviceColors>> {
    ensure!(
        cx.has_global::<DevicePrefs>(),
        "Frontend device configuration provider is unavailable"
    );
    Ok(cx.global::<DevicePrefs>().0.clone())
}

#[cfg(test)]
mod tests;
