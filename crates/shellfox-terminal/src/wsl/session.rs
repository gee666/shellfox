use super::{
    config::NativeWslCapability, protocol::Frame, tunnel::Tunnel, GuestSettings, GuestStatus,
    OsWsl, RecoveryEvidence, WslConfig, WslConsent, WslError, WslProfileId,
};
use crate::{ExitStatus, PtyOutput};
use std::{
    sync::{
        mpsc::{self, SyncSender, TryRecvError},
        Mutex,
    },
    thread,
    time::Duration,
};
#[path = "owner.rs"]
mod owner;
use owner::Owner;

pub struct WslGuestGuardian;
impl WslGuestGuardian {
    /// Explicit opt-in operation. Never called by distro discovery or profile listing.
    pub fn preflight(
        executable: &OsWsl,
        distro: &WslProfileId,
        settings: &GuestSettings,
        _consent: WslConsent,
    ) -> Result<NativeWslCapability, WslError> {
        #[cfg(not(windows))]
        {
            let _ = (executable, distro, settings);
            Err(WslError::Unavailable)
        }
        #[cfg(windows)]
        {
            if !executable.list()?.contains(distro) {
                return Err(WslError::Unavailable);
            }
            Self::probe_native(executable, distro, settings)
        }
    }
    fn probe_native(
        executable: &OsWsl,
        distro: &WslProfileId,
        settings: &GuestSettings,
    ) -> Result<NativeWslCapability, WslError> {
        let mut tunnel = Tunnel::spawn(executable.guest_command(distro, settings))?;
        tunnel.send(serde_json::json!({"mode":"probe"}))?;
        let proof = match tunnel.next()? {
            Frame::Capability {
                protocol: 1,
                boot,
                pidfd: true,
                subreaper: true,
                peercred: true,
            } if super::protocol::boot_id(&boot) => boot,
            _ => return Err(WslError::Unavailable),
        };
        tunnel.finish()?;
        Ok(NativeWslCapability {
            distro: distro.clone(),
            settings: settings.clone(),
            boot: proof,
            executable: executable.clone(),
        })
    }
    pub fn status(
        executable: &OsWsl,
        evidence: &RecoveryEvidence,
    ) -> Result<GuestStatus, WslError> {
        match Self::recover(executable, evidence, "status")? {
            Frame::Reply { status, .. } => Ok(status),
            _ => Err(WslError::Protocol),
        }
    }
    pub fn close(executable: &OsWsl, evidence: &RecoveryEvidence) -> Result<(), WslError> {
        match Self::recover(executable, evidence, "close")? {
            Frame::Reply { status, .. } if status.cleanup_confirmed && status.owned == 0 => Ok(()),
            Frame::Aborted {
                cleanup_confirmed: true,
                owned: 0,
                ..
            } => Ok(()),
            _ => Err(WslError::CleanupUnconfirmed),
        }
    }
    fn recover(
        executable: &OsWsl,
        evidence: &RecoveryEvidence,
        action: &str,
    ) -> Result<Frame, WslError> {
        let mut tunnel =
            Tunnel::spawn(executable.guest_command(&evidence.distro, &evidence.settings))?;
        tunnel.send(serde_json::json!({"mode":"recover","endpoint":evidence.endpoint,"secret":evidence.secret.encoded(),"anchor":evidence.anchor,"action":action}))?;
        let response = tunnel.next()?;
        match &response {
            Frame::Reply { id: 1, status } => {
                status.identity.validate()?;
                if !evidence.anchor.matches(&status.identity) {
                    return Err(WslError::Protocol);
                }
                status.validate(evidence.identity.as_ref().unwrap_or(&status.identity))?;
            }
            Frame::Aborted {
                anchor,
                owned: 0,
                cleanup_confirmed: true,
            } if anchor == &evidence.anchor => {}
            _ => return Err(WslError::Protocol),
        }
        tunnel.finish()?;
        Ok(response)
    }
}

pub struct WslSession {
    inner: Option<Owner>,
    reaper: SyncSender<Owner>,
}
impl std::fmt::Debug for WslSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WslSession")
            .field("retained", &self.inner.is_some())
            .finish()
    }
}
#[derive(Debug)]
pub struct WslSpawnError {
    pub error: WslError,
    session: Mutex<Option<Box<WslSession>>>,
}
impl WslSpawnError {
    pub fn take_session(&self) -> Option<WslSession> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|v| *v)
    }
}
impl std::fmt::Display for WslSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for WslSpawnError {}
impl From<WslError> for WslSpawnError {
    fn from(error: WslError) -> Self {
        Self {
            error,
            session: Mutex::new(None),
        }
    }
}
impl WslSession {
    pub fn spawn(
        config: WslConfig,
        capability: NativeWslCapability,
    ) -> Result<Self, WslSpawnError> {
        #[cfg(not(windows))]
        {
            let _ = (config, capability);
            Err(WslError::Unavailable.into())
        }
        #[cfg(windows)]
        {
            Self::spawn_native(config, capability)
        }
    }
    fn spawn_native(
        config: WslConfig,
        capability: NativeWslCapability,
    ) -> Result<Self, WslSpawnError> {
        config.validate()?;
        if config.distro != capability.distro
            || config.settings != capability.settings
            || config.executable != capability.executable
        {
            return Err(WslError::InvalidRequest.into());
        }
        let (reaper, rx) = mpsc::sync_channel::<Owner>(1);
        thread::Builder::new()
            .name("wsl-retained-guest-owner".into())
            .spawn(move || {
                while let Ok(mut owner) = rx.recv() {
                    while owner.shutdown().is_err() {
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            })
            .map_err(|_| WslError::Unavailable)?;
        let tunnel = Tunnel::spawn(
            config
                .executable
                .guest_command(&config.distro, &config.settings),
        )?;
        let mut session = Self {
            inner: Some(Owner::new(config, tunnel)),
            reaper,
        };
        let startup = session
            .inner
            .as_mut()
            .ok_or(WslError::Io)?
            .start(&capability.boot);
        if let Err(error) = startup {
            if session.shutdown().is_err() {
                return Err(WslSpawnError {
                    error,
                    session: Mutex::new(Some(Box::new(session))),
                });
            }
            return Err(error.into());
        }
        Ok(session)
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), WslError> {
        let owner = self.inner.as_mut().ok_or(WslError::Io)?;
        if !owner.ready || owner.closing {
            return Err(WslError::Unavailable);
        }
        let result = owner.tunnel.input(bytes);
        if matches!(result, Err(WslError::Io | WslError::Protocol)) {
            owner.ready = false;
            owner.closing = true;
        }
        result
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), WslError> {
        super::config::dimensions(cols, rows)?;
        let owner = self.inner.as_ref().ok_or(WslError::Io)?;
        if !owner.ready || owner.closing {
            return Err(WslError::Unavailable);
        }
        owner
            .tunnel
            .send(serde_json::json!({"action":"resize","cols":cols,"rows":rows}))
    }
    pub fn try_read(&self) -> Result<Option<PtyOutput>, TryRecvError> {
        self.inner
            .as_ref()
            .ok_or(TryRecvError::Disconnected)?
            .tunnel
            .read()
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, WslError> {
        let Some(owner) = self.inner.as_mut() else {
            return Ok(None);
        };
        owner.poll()?;
        Ok(owner
            .status
            .as_ref()
            .and_then(|s| s.root_exit)
            .map(ExitStatus::with_exit_code))
    }
    pub fn recovery(&self) -> Option<RecoveryEvidence> {
        self.inner.as_ref().and_then(|s| s.recovery.clone())
    }
    pub fn begin_shutdown(&mut self) -> Result<(), WslError> {
        if let Some(owner) = self.inner.as_mut() {
            owner.close_guest()?;
        }
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<(), WslError> {
        if let Some(owner) = self.inner.as_mut() {
            owner.shutdown()?;
        }
        self.inner = None;
        Ok(())
    }
}
impl Drop for WslSession {
    fn drop(&mut self) {
        if let Some(mut owner) = self.inner.take() {
            if owner.shutdown().is_err() {
                if let Err(mpsc::SendError(retained)) = self.reaper.send(owner) {
                    std::mem::forget(retained);
                }
            }
        }
    }
}

#[cfg(feature = "test-support")]
impl WslGuestGuardian {
    /// Fixture-only injection; does not enable the production WSL profile gate.
    pub fn fixture_preflight(
        executable: &OsWsl,
        distro: &WslProfileId,
        settings: &GuestSettings,
    ) -> Result<NativeWslCapability, WslError> {
        Self::probe_native(executable, distro, settings)
    }
}
#[cfg(feature = "test-support")]
impl WslSession {
    pub fn fixture_spawn(
        config: WslConfig,
        capability: NativeWslCapability,
    ) -> Result<Self, WslSpawnError> {
        Self::spawn_native(config, capability)
    }
}
