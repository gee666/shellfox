use super::{RawCommand, RawOutput, WriteReceipt};
use anyhow::Result;
use std::path::Path;

/// Raw-pipe processes need their own lifecycle supervisor, which this platform
/// doesn't have yet. The Unix PTY supervisor's foreground-TTY startup is not a
/// substitute.
#[derive(Debug)]
pub enum RawProcess {}
#[derive(Debug)]
pub struct RawSpawnError(anyhow::Error);
impl std::fmt::Display for RawSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for RawSpawnError {}
impl RawSpawnError {
    pub fn take_process(&self) -> Option<RawProcess> {
        None
    }
}
impl RawProcess {
    pub fn spawn(
        _supervisor: &Path,
        command: RawCommand,
    ) -> std::result::Result<Self, RawSpawnError> {
        command.resolved().map_err(RawSpawnError)?;
        Err(RawSpawnError(anyhow::anyhow!(
            "Automation processes are not supported on this platform yet."
        )))
    }
    pub fn spawn_with_launch_binding(
        supervisor: &Path,
        command: RawCommand,
        _binding: crate::launch_privilege::NativeLaunchBinding,
    ) -> std::result::Result<Self, RawSpawnError> {
        Self::spawn(supervisor, command)
    }
    pub fn take_launch_evidence(
        &mut self,
    ) -> Option<crate::launch_privilege::NativeLaunchEvidence> {
        match *self {}
    }
    pub fn pid(&self) -> u32 {
        match *self {}
    }
    pub fn write(&mut self, _bytes: Vec<u8>, _eof: bool) -> Result<WriteReceipt> {
        match *self {}
    }
    pub fn release_stdin(&mut self) {
        match *self {}
    }
    pub fn try_output(&self) -> Option<RawOutput> {
        match *self {}
    }
    pub fn poll_exit(&mut self) -> Result<Option<i32>> {
        match *self {}
    }
    pub fn request_stop(&mut self) -> Result<()> {
        match *self {}
    }
    pub fn was_forcibly_stopped(&self) -> bool {
        match *self {}
    }
    pub fn exit_code(&self) -> Option<i32> {
        match *self {}
    }
    pub fn close(&mut self) -> Result<()> {
        match *self {}
    }
}
