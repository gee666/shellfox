//! Ordinary byte pipes under the reviewed cgroup-v2/original-controller boundary.
mod exec;
mod input;
mod process;
mod state;
mod supervisor;
use crate::launch_privilege::{NativeLaunchBinding, NativeLaunchEvidence};
use crate::raw_process::{RawCommand, RawOutput, WriteReceipt};
use anyhow::{Context, Result};
use state::Inner;
use std::{
    path::Path,
    sync::{
        mpsc::{self, SyncSender},
        Mutex,
    },
};
pub(super) const FLAG: &str = "--shellfox-linux-raw-v1";
pub(crate) fn supervisor_entry() -> Option<u32> {
    supervisor::entry()
}
pub struct RawProcess {
    inner: Option<Inner>,
    reaper: SyncSender<Inner>,
    pid: u32,
    input_ended: bool,
    ready: bool,
    stopped: bool,
    closed_status: Option<i32>,
    closed_signal9: bool,
    launch_evidence: Option<NativeLaunchEvidence>,
}
impl std::fmt::Debug for RawProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawProcess")
            .field("pid", &self.pid)
            .field("closed", &self.inner.is_none())
            .finish()
    }
}
#[derive(Debug)]
pub struct RawSpawnError {
    error: anyhow::Error,
    retained: Mutex<Option<Box<RawProcess>>>,
}
impl RawSpawnError {
    pub fn take_process(&self) -> Option<RawProcess> {
        self.retained
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|p| *p)
    }
}
impl std::fmt::Display for RawSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for RawSpawnError {}
impl From<anyhow::Error> for RawSpawnError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            error,
            retained: Mutex::new(None),
        }
    }
}
impl RawProcess {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn take_launch_evidence(&mut self) -> Option<NativeLaunchEvidence> {
        self.launch_evidence.take()
    }
    pub fn write(&mut self, bytes: Vec<u8>, eof: bool) -> Result<WriteReceipt> {
        use crate::raw_process::{AutomationStdinClosed, AutomationStdinUnavailable};
        if self.input_ended {
            return Err(AutomationStdinClosed::known().into());
        }
        let inner = self.inner.as_ref().context("Raw owner unavailable")?;
        if inner.guest.is_some() {
            return Err(AutomationStdinClosed::known().into());
        }
        if !self.ready {
            return Err(AutomationStdinUnavailable::WriterNotReady.into());
        }
        let receipt = inner
            .input
            .as_ref()
            .ok_or(AutomationStdinUnavailable::WriterNotReady)?
            .write(bytes, eof)?;
        if eof {
            self.input_ended = true;
        }
        Ok(receipt)
    }
    pub fn release_stdin(&mut self) {
        self.input_ended = true;
        if let Some(input) = self.inner.as_ref().and_then(|inner| inner.input.as_ref()) {
            input.close();
        }
    }
    pub fn try_output(&self) -> Option<RawOutput> {
        self.inner.as_ref().and_then(|inner| inner.output.read())
    }
    /// Calls `wake` after every output frame and whenever `poll_exit` may have
    /// news, never under a lock. Fires once now for anything earlier. Call once.
    pub fn set_output_wake(&self, wake: crate::Wake) -> Result<()> {
        let inner = self.inner.as_ref().context("Raw owner unavailable")?;
        inner.output.set_wake(wake.clone());
        // The controller is in the group and exits only after reporting the
        // guest's status, so "empty" is when poll_exit can finish.
        inner.group.watch(wake)
    }
    pub fn poll_exit(&mut self) -> Result<Option<i32>> {
        self.inner.as_mut().map_or(Ok(None), Inner::poll)
    }
    pub fn request_stop(&mut self) -> Result<()> {
        self.release_stdin();
        if let Some(inner) = &mut self.inner {
            self.stopped |= inner.stop()?;
        }
        Ok(())
    }
    pub fn was_forcibly_stopped(&self) -> bool {
        self.stopped
            && self.exit_code() == Some(137)
            && self
                .inner
                .as_ref()
                .map_or(self.closed_signal9, |i| i.guest.is_some_and(|g| g.signal9))
    }
    pub fn exit_code(&self) -> Option<i32> {
        self.closed_status
            .or_else(|| self.inner.as_ref().and_then(|i| i.guest.map(|g| g.code)))
    }
    pub fn close(&mut self) -> Result<()> {
        self.ready = false;
        self.release_stdin();
        if let Some(inner) = &mut self.inner {
            inner.close()?;
            self.closed_status = inner.guest.map(|status| status.code);
            self.closed_signal9 = inner.guest.is_some_and(|status| status.signal9);
        }
        self.inner = None;
        Ok(())
    }
    #[cfg(feature = "test-support")]
    pub fn signal_controller_for_test(&self, signal: i32) -> Result<()> {
        use std::os::fd::AsRawFd;
        anyhow::ensure!(
            matches!(
                signal,
                libc::SIGHUP | libc::SIGTERM | libc::SIGINT | libc::SIGKILL
            ),
            "invalid controller signal"
        );
        let fd = self
            .inner
            .as_ref()
            .and_then(|inner| inner.pidfd.as_ref())
            .context("missing original pidfd")?;
        anyhow::ensure!(
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    fd.as_raw_fd(),
                    signal,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            } == 0,
            "signal original raw controller"
        );
        Ok(())
    }
}
impl Drop for RawProcess {
    fn drop(&mut self) {
        if let Some(mut inner) = self.inner.take() {
            if inner.close().is_err() {
                if let Err(mpsc::SendError(retained)) = self.reaper.send(inner) {
                    std::mem::forget(retained);
                }
            }
        }
    }
}
