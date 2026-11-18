use super::input::Input;
use crate::{raw_process::io::Output, unix::cgroup::Cgroup};
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    process::{Child, ExitStatus},
    thread::JoinHandle,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
pub(super) struct GuestExit {
    pub code: i32,
    pub signal9: bool,
}
pub(super) struct Inner {
    pub child: Child,
    pub pidfd: Option<File>,
    pub group: Cgroup,
    pub assigned: bool,
    pub control: Option<UnixStream>,
    pub input: Option<Input>,
    pub output: Output,
    pub readers: Vec<JoinHandle<()>>,
    pub guest: Option<GuestExit>,
    root: Option<ExitStatus>,
    frame: Vec<u8>,
    stop_sent: bool,
    removed: bool,
}
impl Inner {
    pub fn new(child: Child, group: Cgroup) -> Self {
        Self {
            child,
            group,
            pidfd: None,
            assigned: false,
            control: None,
            input: None,
            output: Output::default(),
            readers: vec![],
            guest: None,
            root: None,
            frame: Vec::new(),
            stop_sent: false,
            removed: false,
        }
    }
    pub fn pin(&mut self) -> Result<()> {
        // Original Child is held unreaped, never a later lookup by a cached PID.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, self.child.id(), 0) };
        ensure!(
            fd >= 0,
            "pin original raw controller: {}",
            std::io::Error::last_os_error()
        );
        self.pidfd = Some(unsafe { File::from_raw_fd(i32::try_from(fd)?) });
        Ok(())
    }
    pub fn attach(&mut self) -> Result<()> {
        self.group.attach(self.child.id())?;
        self.assigned = true;
        Ok(())
    }
    fn read_report(&mut self) -> Result<()> {
        if self.guest.is_none() {
            if let Some(control) = &mut self.control {
                let mut bytes = [0; 5];
                loop {
                    match control.read(&mut bytes[..5 - self.frame.len()]) {
                        Ok(0) => break,
                        Ok(count) => {
                            self.frame.extend_from_slice(&bytes[..count]);
                            if self.frame.len() == 5 {
                                let code = i32::from_le_bytes(self.frame[..4].try_into()?);
                                ensure!(
                                    (0..=255).contains(&code)
                                        && self.frame[4] <= 1
                                        && (self.frame[4] == 0 || code == 137),
                                    "invalid guest exit report"
                                );
                                self.guest = Some(GuestExit {
                                    code,
                                    signal9: self.frame[4] == 1,
                                });
                                break;
                            }
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) =>
                        {
                            break
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        Ok(())
    }
    fn observe(&mut self) -> Result<()> {
        self.read_report()?;
        if self.root.is_none() {
            self.root = self.child.try_wait()?;
        }
        // A report may arrive between the first nonblocking read and waitpid.
        // Drain again after original-root exit before declaring status unknown.
        if self.root.is_some() {
            self.read_report()?;
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Option<i32>> {
        self.observe()?;
        if self.guest.is_some() || self.root.is_some() {
            if let Some(input) = &self.input {
                input.close();
            }
            if self.assigned {
                self.group.terminate()?;
            }
            if self.root.is_some() && self.group.empty()? {
                // Controller SIGKILL is cleanup, never the guest's exit code.
                return self
                    .guest
                    .map(|guest| Some(guest.code))
                    .context("raw guest exit status unconfirmed; original owner retained");
            }
        }
        Ok(None)
    }
    pub fn stop(&mut self) -> Result<bool> {
        self.observe()?;
        if self.guest.is_some() || self.root.is_some() || self.stop_sent {
            return Ok(false);
        }
        if let Some(control) = &mut self.control {
            control
                .write_all(b"K")
                .context("request original raw guest SIGKILL")?;
            self.stop_sent = true;
            return Ok(true);
        }
        Ok(false)
    }
    pub fn close(&mut self) -> Result<()> {
        if self.removed {
            return Ok(());
        }
        self.output.discard();
        if let Some(input) = &self.input {
            input.close();
        }
        if self.assigned && self.control.is_some() {
            if self.stop().is_err() {
                self.group.terminate()?;
            }
        } else if self.assigned {
            self.group.terminate()?;
        } else if let Some(fd) = &self.pidfd {
            // Only the trusted unreleased controller exists before GO.
            let ok = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    fd.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            ensure!(
                ok == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
                "stop unreleased raw controller"
            );
        }
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            self.observe()?;
            if self.assigned && (self.guest.is_some() || self.root.is_some()) {
                self.group.terminate()?;
            }
            let input_done = self.input.as_mut().map_or(Ok(true), Input::join)?;
            if self.root.is_some()
                && self.group.empty()?
                && input_done
                && self.readers.iter().all(JoinHandle::is_finished)
            {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "raw cleanup unconfirmed; original owners retained"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        for reader in self.readers.drain(..) {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("raw output worker panicked"))?;
        }
        self.group.remove()?;
        self.removed = true;
        Ok(())
    }
}
