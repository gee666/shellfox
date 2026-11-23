//! Adapter for the active C guardian supervisor, not the retired native backend.
mod control;
mod preflight;

use super::private::{helper_path, random_token, PrivateDirectory};
use super::{ControlCommand, ControlIdentity, Ownership, SupervisorStatus};
use anyhow::{ensure, Context, Result};
use control::Control;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

pub struct SupervisorLaunch {
    pub file: PathBuf,
    pub args: Vec<OsString>,
    /// Merged over the terminal environment built by the host.
    pub env: BTreeMap<OsString, OsString>,
}

#[must_use = "retain supervisor control until owned cleanup is confirmed"]
pub struct PreparedSupervisor {
    helper: PathBuf,
    arch: String,
    control: Option<Control>,
    reaper: Sender<Control>,
}
impl PreparedSupervisor {
    /// `helper` comes from the verified resource manifest, never PATH. The build
    /// recipe retains the active C source and emits a checksum for packaging.
    pub fn prepare(helper: &Path, runtime_directory: &Path, expected_arch: &str) -> Result<Self> {
        let helper = helper_path(helper)?;
        preflight::check(&helper, expected_arch)?;
        let directory = PrivateDirectory::create(runtime_directory)?;
        let control = Control {
            directory,
            token: random_token()?,
            supervisor_pid: None,
            identity: None,
        };
        let (sender, receiver) = mpsc::channel::<Control>();
        std::thread::Builder::new()
            .name("darwin-owned-cleanup".into())
            .spawn(move || {
                while let Ok(mut control) = receiver.recv() {
                    while control.close().is_err() {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })?;
        Ok(Self {
            helper,
            arch: expected_arch.to_owned(),
            control: Some(control),
            reaper: sender,
        })
    }

    pub fn launch(
        &self,
        command: &Path,
        args: &[OsString],
        marker: &str,
    ) -> Result<SupervisorLaunch> {
        let control = self.control.as_ref().context("supervisor is closed")?;
        ensure!(
            control.supervisor_pid.is_none(),
            "supervisor was already launched"
        );
        ensure!(
            marker.len() == 36
                && marker.bytes().enumerate().all(|(i, b)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                }),
            "invalid terminal marker"
        );
        let command = helper_path(command)?;
        let mut argv = vec![
            "--supervise".into(),
            control.directory.socket().into_os_string(),
            command.into_os_string(),
        ];
        argv.extend_from_slice(args);
        Ok(SupervisorLaunch {
            file: self.helper.clone(),
            args: argv,
            env: BTreeMap::from([
                (
                    "SHELLFOX_SUPERVISOR_TOKEN".into(),
                    control.token.clone().into(),
                ),
                ("SHELLFOX_SUPERVISOR_MARKER".into(), marker.into()),
            ]),
        })
    }

    /// Bind the original portable-pty child PID, not a cached process-tree PID.
    /// The caller must retain its direct child handle until verified shutdown.
    pub fn bind_pid(&mut self, supervisor_pid: u32) -> Result<()> {
        ensure!(
            supervisor_pid > 0 && supervisor_pid <= i32::MAX as u32,
            "invalid supervisor PID"
        );
        let control = self.control.as_mut().context("supervisor is closed")?;
        ensure!(
            control.supervisor_pid.is_none(),
            "supervisor identity already bound"
        );
        control.supervisor_pid = Some(supervisor_pid);
        Ok(())
    }
    pub fn identity(&self) -> Option<&ControlIdentity> {
        self.control.as_ref()?.identity.as_ref()
    }
    pub fn status(&self) -> Result<SupervisorStatus> {
        self.control
            .as_ref()
            .context("supervisor is closed")?
            .request(ControlCommand::Status, Duration::from_secs(15))
    }
    /// Only the original retained PTY child handle may supply this status. The
    /// active C helper exits normally only before child launch or after cleanup.
    #[cfg(feature = "test-support")]
    pub(crate) fn original_child_exit(&mut self, status: &portable_pty::ExitStatus) -> Result<()> {
        ensure!(
            status.signal().is_none(),
            "signalled supervisor exit does not prove cleanup"
        );
        if let Some(control) = &self.control {
            ensure!(control.supervisor_pid.is_some(), "supervisor was not bound");
        }
        self.control = None;
        Ok(())
    }
    pub fn close(&mut self) -> Result<()> {
        if let Some(control) = self.control.as_mut() {
            if control.supervisor_pid.is_some() {
                control.close()?;
            }
        }
        self.control = None;
        Ok(())
    }
}
impl Ownership for PreparedSupervisor {
    fn prepare(&mut self) -> Result<()> {
        ensure!(
            self.control
                .as_ref()
                .is_some_and(|c| c.supervisor_pid.is_none()),
            "supervisor is not prepared"
        );
        preflight::check(&self.helper, &self.arch)
    }
    fn authenticate(&mut self) -> Result<()> {
        self.control
            .as_mut()
            .context("supervisor is closed")?
            .ready(Duration::from_secs(15))
    }
    fn close(&mut self) -> Result<()> {
        PreparedSupervisor::close(self)
    }
}
impl Drop for PreparedSupervisor {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            if control.supervisor_pid.is_some() {
                if let Err(mpsc::SendError(retained)) = self.reaper.send(control) {
                    std::mem::forget(retained);
                }
            }
        }
    }
}
