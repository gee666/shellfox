use anyhow::{Context, Result, bail};
pub(crate) use shellfox_service::app_composition;
mod launch;
#[cfg(test)]
mod launch_tests;
use std::{ffi::OsString, path::PathBuf, time::SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Fixture,
    Shell,
    Smoke,
}

#[derive(Clone)]
pub struct Options {
    pub mode: Mode,
    pub launch: crate::app_launch::DesktopLaunchIntent,
    pub ui_acceptance: bool,
    pub data_dir: PathBuf,
    pub desktop_dir: PathBuf,
    pub device_config_root: PathBuf,
    pub run_dir: PathBuf,
    pub endpoint: Option<crate::host_client::HostEndpointConfig>,
    /// Raw application flags, forwarded to the host process.
    pub application_args: Vec<OsString>,
    /// Persistent flags, without launch intents, for a UI relaunched by its host.
    pub relaunch_args: Vec<OsString>,
    /// Set by a host that relaunches its crashed UI: Quit stops that host.
    pub owned_host: bool,
    /// The crashed UI's report, passed by the host that relaunched it.
    pub crash_report: Option<PathBuf>,
}

impl Options {
    pub(crate) fn for_window(&self) -> Result<Self> {
        let desktop = &self.desktop_dir;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let mut options = self.clone();
        options.run_dir = desktop.join(format!("run-{}-{stamp}", std::process::id()));
        Ok(options)
    }
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter().peekable();
        let mut mode = Mode::Shell;
        let mut ui_acceptance = false;
        let mut data_dir = None;
        let mut desktop_dir = None;
        let mut device_config_root = None;
        let mut helper = None;
        let mut helper_sha256 = None;
        let mut application = app_composition::ApplicationArguments::default();
        let mut launch_args = Vec::new();
        let mut application_args = Vec::new();
        let mut relaunch_args = Vec::new();
        let mut owned_host = false;
        let mut crash_report = None;
        while let Some(arg) = args.next() {
            if app_composition::ApplicationArguments::is_flag(&arg) {
                let value = args.next().context("Application flag requires a value")?;
                application.push(
                    arg.to_str().context("Application flag must be UTF-8")?,
                    value.clone(),
                )?;
                application_args.extend([arg.clone(), value.clone()]);
                relaunch_args.extend([arg, value]);
                continue;
            }
            if matches!(
                arg.to_str(),
                Some(
                    "--helper"
                        | "--helper-sha256"
                        | "--device-config-root"
                        | "--desktop-dir"
                        | "--data-dir"
                )
            ) && let Some(value) = args.peek()
            {
                relaunch_args.extend([arg.clone(), value.clone()]);
            }
            match arg.to_str() {
                Some("--fixture") => mode = Mode::Fixture,
                Some("--ui-acceptance") => {
                    ui_acceptance = true;
                    relaunch_args.push("--ui-acceptance".into());
                }
                Some("--owned-host") => owned_host = true,
                Some("--crash-report") => {
                    crash_report = Some(PathBuf::from(
                        args.next().context("--crash-report requires a path")?,
                    ));
                }
                Some("--helper") => {
                    if helper.is_some() {
                        bail!("--helper may only be supplied once");
                    }
                    helper = Some(PathBuf::from(
                        args.next()
                            .context("--helper requires an absolute executable path")?,
                    ));
                }
                Some("--helper-sha256") => {
                    if helper_sha256.is_some() {
                        bail!("--helper-sha256 may only be supplied once");
                    }
                    let value = args
                        .next()
                        .context("--helper-sha256 requires the trusted build digest")?;
                    helper_sha256 = Some(parse_digest(
                        value
                            .to_str()
                            .context("helper digest must be hexadecimal text")?,
                    )?);
                }
                Some("--new-session" | "--new-window") => launch_args.push(
                    arg.into_string()
                        .map_err(|_| anyhow::anyhow!("Launch arguments must be UTF-8"))?,
                ),
                Some(flag @ ("--cwd" | "--launch-request-id")) => {
                    launch_args.push(flag.to_owned());
                    launch_args.push(
                        args.next()
                            .with_context(|| format!("{flag} requires a value"))?
                            .into_string()
                            .map_err(|_| anyhow::anyhow!("Launch arguments must be UTF-8"))?,
                    );
                }
                Some("start" | "add" | "new" | "create") => {
                    launch_args.push(
                        arg.into_string()
                            .map_err(|_| anyhow::anyhow!("Launch arguments must be UTF-8"))?,
                    );
                    if args
                        .peek()
                        .is_some_and(|value| !value.to_string_lossy().starts_with("--"))
                    {
                        launch_args.push(
                            args.next()
                                .context("Launch path is missing")?
                                .into_string()
                                .map_err(|_| anyhow::anyhow!("Launch path must be UTF-8"))?,
                        );
                    }
                }
                Some("--shell") => mode = Mode::Shell,
                Some("--smoke") => mode = Mode::Smoke,
                Some("--device-config-root") => {
                    if device_config_root.is_some() {
                        bail!("--device-config-root may only be supplied once");
                    }
                    device_config_root = Some(PathBuf::from(args.next().context(
                        "--device-config-root requires an absolute frontend directory",
                    )?));
                }
                Some("--desktop-dir") => {
                    if desktop_dir.is_some() {
                        bail!("--desktop-dir may only be supplied once");
                    }
                    desktop_dir = Some(PathBuf::from(
                        args.next()
                            .context("--desktop-dir requires an absolute UI scratch directory")?,
                    ));
                }
                Some("--data-dir") => {
                    data_dir = Some(PathBuf::from(
                        args.next()
                            .context("--data-dir requires an absolute directory")?,
                    ))
                }
                _ => bail!("usage: shellfox [--data-dir PATH] [--fixture|--shell|--smoke]"),
            }
        }
        let root = match data_dir {
            Some(root) => root,
            None if mode == Mode::Shell => shellfox_service::app_paths::default_data_root()?,
            None => bail!("--fixture and --smoke require an explicit --data-dir"),
        };
        if !root.is_absolute() {
            bail!("--data-dir must be an absolute path");
        }
        if root.parent().is_none() {
            bail!("--data-dir must not be a filesystem root");
        }
        let desktop = if let Some(desktop) = desktop_dir {
            desktop
        } else {
            let mut name = root
                .file_name()
                .context("--data-dir must name a directory")?
                .to_os_string();
            name.push(".desktop");
            root.with_file_name(name)
        };
        if !desktop.is_absolute() || desktop.starts_with(&root) {
            bail!("--desktop-dir must be absolute and outside the dedicated host data directory");
        }
        let device_config_root =
            crate::device_prefs::configured_root(&root, &desktop, device_config_root)?;
        let launch = launch::parse(&launch_args, mode)?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let run_dir = desktop.join(format!("run-{}-{stamp}", std::process::id()));
        let endpoint = match (helper, helper_sha256) {
            (Some(helper), Some(helper_sha256)) => {
                if !helper.is_absolute() {
                    bail!("--helper must be absolute");
                }
                Some(crate::host_client::HostEndpointConfig {
                    data_dir: root.clone(),
                    helper,
                    helper_sha256,
                })
            }
            (None, None)
                if mode == Mode::Shell
                    && !cfg!(test)
                    && shellfox_service::app_paths::is_packaged() =>
            {
                let (helper, helper_sha256) = shellfox_service::app_paths::packaged_helper()?;
                Some(crate::host_client::HostEndpointConfig {
                    data_dir: root.clone(),
                    helper,
                    helper_sha256,
                })
            }
            (None, None) => None,
            _ => bail!("--helper and --helper-sha256 must be supplied together"),
        };
        // Validated here so a bad flag fails before a host is started.
        let application = application.finish(&root)?;
        application.validate_separate(&[&desktop, &device_config_root])?;
        Ok(Self {
            mode,
            launch,
            ui_acceptance,
            data_dir: root,
            desktop_dir: desktop,
            device_config_root,
            run_dir,
            endpoint,
            application_args,
            relaunch_args,
            owned_host,
            crash_report,
        })
    }
}

fn parse_digest(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("--helper-sha256 requires exactly 64 hexadecimal digits from a trusted helper build");
    }
    let mut digest = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_is_default_and_store_root_survives_run_rotation() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("tmp")
            .join(format!(
                "desktop-options-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let first =
            Options::parse([OsString::from("--data-dir"), root.clone().into_os_string()]).unwrap();
        let second = Options::parse([
            OsString::from("--fixture"),
            OsString::from("--ui-acceptance"),
            OsString::from("--data-dir"),
            root.clone().into_os_string(),
        ])
        .unwrap();
        assert_eq!(first.mode, Mode::Shell);
        assert_eq!(second.mode, Mode::Fixture);
        assert!(!first.ui_acceptance);
        assert!(second.ui_acceptance);
        assert_eq!(first.data_dir, second.data_dir);
        assert_ne!(first.run_dir, second.run_dir);
        assert!(!first.run_dir.starts_with(&root));
        let desktop = first.run_dir.parent().unwrap().to_owned();
        assert_eq!(desktop, second.run_dir.parent().unwrap());
        assert_eq!(first.desktop_dir, desktop);
        let another_window = first.for_window().unwrap();
        assert_eq!(another_window.desktop_dir, first.desktop_dir);
        assert_eq!(another_window.device_config_root, first.device_config_root);
        assert_ne!(another_window.run_dir, first.run_dir);
        assert!(
            !root.exists(),
            "Planning must not create a private Host root"
        );
        assert!(
            !desktop.exists(),
            "Planning/for_window must not create frontend scratch"
        );
    }
    #[test]
    fn default_uses_the_rust_root_and_overrides_must_be_absolute() {
        let options = Options::parse([]).unwrap();
        assert_eq!(
            options.data_dir,
            shellfox_service::app_paths::default_data_root().unwrap()
        );
        assert!(Options::parse(["--data-dir".into(), "relative".into()]).is_err());
        assert!(Options::parse(["--unknown".into()]).is_err());
    }
}
