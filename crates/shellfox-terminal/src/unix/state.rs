use super::cgroup::Cgroup;
use crate::{input::InputWriter, reader::OutputReader, ExitStatus};
use anyhow::{ensure, Context, Result};
use portable_pty::{Child, MasterPty};
use std::time::{Duration, Instant};
use std::{
    fs::File,
    io::Read,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::net::UnixStream,
};

pub(super) struct State {
    pub group: Option<Cgroup>,
    pub child: Option<Box<dyn Child + Send + Sync>>,
    pub pidfd: Option<File>,
    pub attached: bool,
    pub control: Option<UnixStream>,
    pub master: Option<Box<dyn MasterPty + Send>>,
    pub writer: InputWriter,
    pub output: OutputReader,
    pub evidence: Option<crate::NativeLaunchEvidence>,
    pub stopping: bool,
    pub ready: bool,
    #[cfg(feature = "test-support")]
    pub inject_cleanup_failure: bool,
    logical: Option<ExitStatus>,
    root: Option<ExitStatus>,
    exit_frame: Vec<u8>,
}
impl State {
    pub fn new(
        group: Cgroup,
        master: Box<dyn MasterPty + Send>,
        writer: InputWriter,
        output: OutputReader,
    ) -> Self {
        Self {
            group: Some(group),
            child: None,
            pidfd: None,
            attached: false,
            control: None,
            master: Some(master),
            writer,
            output,
            evidence: None,
            stopping: false,
            ready: false,
            logical: None,
            root: None,
            exit_frame: Vec::new(),
            #[cfg(feature = "test-support")]
            inject_cleanup_failure: false,
        }
    }
    pub fn pin(&mut self) -> Result<u32> {
        let pid = self
            .child
            .as_ref()
            .context("original PTY child missing")?
            .process_id()
            .context("original PTY PID missing")?;
        // Original Child is unreaped, so this PID cannot be recycled.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        ensure!(
            fd >= 0,
            "pin original PTY child: {}",
            std::io::Error::last_os_error()
        );
        self.pidfd = Some(unsafe { File::from_raw_fd(i32::try_from(fd)?) });
        Ok(pid)
    }
    pub fn attach(&mut self, pid: u32) -> Result<()> {
        self.group
            .as_mut()
            .context("PTY cgroup missing")?
            .attach(pid)?;
        self.attached = true;
        Ok(())
    }
    pub fn begin(&mut self) -> Result<()> {
        self.stopping = true;
        self.ready = false;
        self.writer.stop();
        if self.attached {
            self.group
                .as_mut()
                .context("PTY cgroup missing")?
                .terminate()?;
        } else if let Some(pidfd) = &self.pidfd {
            // Before attachment the waiting original child cannot fork/exec.
            let result = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    pidfd.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            ensure!(
                result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
                "terminate waiting original child"
            );
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Option<ExitStatus>> {
        if self.logical.is_none() {
            if let Some(control) = &mut self.control {
                let mut bytes = [0; 4];
                loop {
                    match control.read(&mut bytes[..4 - self.exit_frame.len()]) {
                        Ok(0) => break,
                        Ok(count) => {
                            self.exit_frame.extend_from_slice(&bytes[..count]);
                            if self.exit_frame.len() == 4 {
                                self.logical = Some(ExitStatus::with_exit_code(
                                    u32::from_le_bytes(self.exit_frame.as_slice().try_into()?),
                                ));
                                break;
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) =>
                        {
                            break
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        if self.root.is_none() {
            if let Some(child) = &mut self.child {
                self.root = child.try_wait().context("wait original PTY controller")?;
            }
        }
        if self.logical.is_some() || self.root.is_some() || self.stopping {
            self.begin()?;
            if self.root.is_some() && self.group.as_mut().context("PTY cgroup missing")?.empty()? {
                return Ok(self.logical.clone().or_else(|| self.root.clone()));
            }
        }
        Ok(None)
    }
    pub fn shutdown(&mut self) -> Result<()> {
        self.output.cancel();
        self.begin()?;
        #[cfg(feature = "test-support")]
        ensure!(
            !self.inject_cleanup_failure,
            "injected cleanup uncertainty; owners retained"
        );
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            self.poll()?;
            let root_gone = self.child.is_none() || self.root.is_some();
            if root_gone
                && self.group.as_mut().context("PTY cgroup missing")?.empty()?
                && self.writer.finished()
                && self.output.finished()
            {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "PTY cleanup unconfirmed; original owners retained"
            );
            // Bounded poll: the cgroup, root and pipe workers share no wake.
            std::thread::sleep(Duration::from_millis(2));
        }
        self.writer.join()?;
        self.output.join()?;
        self.group
            .as_mut()
            .context("PTY cgroup missing")?
            .remove()?;
        self.group = None;
        self.control = None;
        self.master = None;
        self.child = None;
        self.pidfd = None;
        Ok(())
    }
}
