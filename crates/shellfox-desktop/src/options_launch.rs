use super::Mode;
use crate::app_launch::{DesktopLaunchIntent, LaunchDirectoryPolicy, LaunchError};
use anyhow::{Context, Result, ensure};
use shellfox_service::desktop_launch::LaunchOwnerFence;

struct DirectoryPreflight;
impl LaunchDirectoryPolicy for DirectoryPreflight {
    fn validate_directory(&self, cwd: &str) -> std::result::Result<String, LaunchError> {
        shellfox_core::validation::cwd(cwd).map_err(|_| LaunchError::Invalid)?;
        // Argument planning only. The actual owner validates existence/access
        // and canonicalizes/revalidates the local/WSL target before mutation.
        Ok(cwd.to_owned())
    }
}

pub(super) fn parse(
    args: &[String],
    attach_existing: bool,
    owner: Option<&str>,
    mode: Mode,
) -> Result<(DesktopLaunchIntent, Option<LaunchOwnerFence>)> {
    ensure!(
        mode == Mode::Shell || args.is_empty() && !attach_existing && owner.is_none(),
        "Desktop launch arguments cannot be used with fixture/smoke mode"
    );
    ensure!(
        attach_existing == owner.is_some(),
        "--attach-existing-host and --launch-owner must be supplied together"
    );
    let owner = owner.map(str::parse::<LaunchOwnerFence>).transpose()?;
    let invoking_cwd = std::env::current_dir().context("Read invoking process directory")?;
    let cwd = invoking_cwd
        .to_str()
        .context("Launch directory must be UTF-8")?;
    let intent = crate::app_launch::parse_launch_args(args, cwd, &DirectoryPreflight)?;
    Ok((intent, owner))
}
