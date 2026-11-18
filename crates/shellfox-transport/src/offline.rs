//! Tracked offline helper commands. Never start tsnet or accept network traffic.
use crate::{offline_control::OfflineControl, offline_scope::OfflineScope, HelperError};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

#[path = "ownership.rs"]
mod ownership;
pub use ownership::HostOwnership;

pub(crate) struct OfflineCommand {
    pub(crate) control: Arc<OfflineControl>,
    stdout: Option<BufReader<tokio::process::ChildStdout>>,
    stderr_done: Arc<AtomicBool>,
    disarmed: bool,
    _scope: OfflineScope,
}
impl OfflineCommand {
    pub fn spawn_in(
        scope: &OfflineScope,
        args: &[&Path],
        protect: bool,
    ) -> Result<Self, HelperError> {
        let control = scope.spawn(args, protect)?;
        // All failures below are post-registration: preparation/monitor still
        // owns the original process and artifact/root resources.
        let (stdout, stderr) = control.pipes()?;
        let stdout =
            tokio::process::ChildStdout::from_std(stdout).map_err(|_| HelperError::Spawn)?;
        let mut stderr =
            tokio::process::ChildStderr::from_std(stderr).map_err(|_| HelperError::Spawn)?;
        let stderr_done = Arc::new(AtomicBool::new(false));
        let done = stderr_done.clone();
        let original = control.clone();
        let task = tokio::spawn(async move {
            let mut scratch = [0; 1024];
            loop {
                match stderr.read(&mut scratch).await {
                    Ok(0) => break,
                    Ok(_) => original.fault(),
                    Err(_) => {
                        original.fault();
                        break;
                    }
                }
            }
            done.store(true, Ordering::Release);
        });
        control.task(task);
        Ok(Self {
            control,
            stdout: Some(BufReader::new(stdout)),
            stderr_done,
            disarmed: false,
            _scope: scope.clone(),
        })
    }
    pub async fn line(&mut self) -> Result<Vec<u8>, HelperError> {
        let reader = self.stdout.as_mut().ok_or(HelperError::Stopped)?;
        let mut line = Vec::new();
        loop {
            let bytes = reader.fill_buf().await.map_err(|_| HelperError::Protocol)?;
            if bytes.is_empty() {
                return Err(HelperError::Protocol);
            }
            let end = bytes.iter().position(|b| *b == b'\n');
            let count = end.map_or(bytes.len(), |n| n + 1);
            if line.len() + count > 1024 {
                return Err(HelperError::Protocol);
            }
            line.extend_from_slice(&bytes[..count]);
            reader.consume(count);
            if end.is_some() {
                return Ok(line);
            }
        }
    }
    pub async fn expect_exit(&mut self) -> Result<(), HelperError> {
        let exit = self.wait_original().await?;
        if !exit.success() {
            return Err(HelperError::Command);
        }
        Ok(())
    }
    pub async fn wait_original(&mut self) -> Result<std::process::ExitStatus, HelperError> {
        let mut extra = [0; 1];
        if self
            .stdout
            .as_mut()
            .ok_or(HelperError::Stopped)?
            .read(&mut extra)
            .await
            .map_err(|_| HelperError::Protocol)?
            != 0
        {
            return Err(HelperError::Protocol);
        }
        let exit = self.control.wait_exit(Duration::from_secs(2)).await?;
        let end = Instant::now() + Duration::from_secs(2);
        while !self.stderr_done.load(Ordering::Acquire) {
            if Instant::now() >= end {
                return Err(HelperError::Deadline);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if self.control.has_fault() {
            return Err(HelperError::Command);
        }
        Ok(exit)
    }
    pub async fn terminate(&mut self) -> Result<(), HelperError> {
        self.control.release(true)?;
        self.control.wait_exit(Duration::from_secs(2)).await?;
        Ok(())
    }
    pub fn hold_ownership(&mut self) -> Result<(), HelperError> {
        if self.control.poll_exit()?.is_some() || self.control.has_fault() {
            return Err(HelperError::Stopped);
        }
        self.control.set_holding()?;
        self.disarmed = true;
        let mut stdout = self.stdout.take().ok_or(HelperError::Stopped)?;
        let original = self.control.clone();
        let task = tokio::spawn(async move {
            let mut scratch = [0; 1024];
            loop {
                match stdout.read(&mut scratch).await {
                    Ok(0) => {
                        if !original.lost().is_cancelled() {
                            original.fault();
                        }
                        break;
                    }
                    Ok(_) => original.fault(),
                    Err(_) => {
                        original.fault();
                        break;
                    }
                }
            }
        });
        self.control.task(task);
        Ok(())
    }
}
impl Drop for OfflineCommand {
    fn drop(&mut self) {
        if !self.disarmed {
            let _ = self.control.release(true);
        }
    }
}
