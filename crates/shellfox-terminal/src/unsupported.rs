use crate::{ExitStatus, PtyConfig, PtyOutput, SpawnError};
use anyhow::{bail, Result};
use std::sync::mpsc::TryRecvError;

const UNSUPPORTED: &str =
    "interactive PTY disabled: no proven owned process-tree boundary on this platform";

#[derive(Debug)]
pub struct PtySession {
    _private: (),
}

impl PtySession {
    pub fn spawn_bound(
        _config: PtyConfig,
        _binding: crate::NativeLaunchBinding,
    ) -> std::result::Result<Self, SpawnError> {
        Err(anyhow::anyhow!(UNSUPPORTED).into())
    }
    pub fn spawn_program_bound(
        _config: PtyConfig,
        _environment: crate::TrustedProgramEnv,
        _binding: crate::NativeLaunchBinding,
    ) -> std::result::Result<Self, SpawnError> {
        Err(anyhow::anyhow!(UNSUPPORTED).into())
    }
    pub fn take_launch_evidence(&mut self) -> Option<crate::NativeLaunchEvidence> {
        None
    }
    pub fn spawn_program(
        _config: PtyConfig,
        _environment: crate::TrustedProgramEnv,
    ) -> std::result::Result<Self, SpawnError> {
        Err(anyhow::anyhow!(UNSUPPORTED).into())
    }
    /// Refuse before allocating a PTY or spawning any process.
    pub fn spawn(_config: PtyConfig) -> std::result::Result<Self, SpawnError> {
        Err(anyhow::anyhow!(UNSUPPORTED).into())
    }
    pub fn try_read(&self) -> std::result::Result<Option<PtyOutput>, TryRecvError> {
        Err(TryRecvError::Disconnected)
    }
    pub fn write(&mut self, _bytes: &[u8]) -> Result<()> {
        bail!(UNSUPPORTED)
    }
    pub fn resize(&self, _cols: u16, _rows: u16) -> Result<()> {
        bail!(UNSUPPORTED)
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        bail!(UNSUPPORTED)
    }
    pub fn begin_shutdown(&mut self) -> Result<()> {
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_never_falls_back_to_a_direct_child() {
        let config = PtyConfig::new("not-even-an-existing-cwd");
        let error = PtySession::spawn(config).unwrap_err();
        assert!(error.to_string().contains("owned process-tree boundary"));
        assert!(error.take_session().is_none());
    }
}
