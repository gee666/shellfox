use super::procfs::{self, bounded, missing, record};
use crate::unix::tree::{Identity, Kernel, Record, Signal};
use anyhow::{ensure, Context, Result};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub(super) struct Pidfd {
    fd: OwnedFd,
}
pub(super) struct LinuxKernel {
    pub uid: u32,
    pub boot: String,
}
impl LinuxKernel {
    pub fn preflight() -> Result<Self> {
        // SAFETY: credential queries have no preconditions.
        let uid = unsafe { libc::getuid() };
        ensure!(
            uid == unsafe { libc::geteuid() },
            "real/effective UID mismatch"
        );
        let mut subreaper: libc::c_int = 0;
        // Query only. The host must not become a process-global subreaper.
        ensure!(
            unsafe {
                libc::prctl(
                    libc::PR_GET_CHILD_SUBREAPER,
                    &mut subreaper as *mut libc::c_int,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                )
            } == 0,
            "Linux subreaper API unavailable"
        );
        let boot = String::from_utf8(bounded("/proc/sys/kernel/random/boot_id", 64)?)?
            .trim()
            .to_owned();
        ensure!(valid_boot(&boot), "invalid Linux boot identity");
        let mut kernel = Self { uid, boot };
        let own = record(std::process::id(), uid)?;
        let handle = kernel.pin(&own)?.context("self pidfd disappeared")?;
        send(&handle, 0)?;
        ensure!(!kernel.exited(&handle)?, "self pidfd unexpectedly exited");
        ensure!(
            kernel
                .snapshot()?
                .iter()
                .any(|row| row.identity == own.identity),
            "bounded process enumeration failed native preflight"
        );
        Ok(kernel)
    }
}
impl Kernel for LinuxKernel {
    type Handle = Pidfd;
    fn snapshot(&mut self) -> Result<Vec<Record>> {
        let mut rows = Vec::new();
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            ensure!(rows.len() < 20000, "process snapshot bound exceeded");
            let row = (|| {
                let bytes = bounded(entry.path().join("stat"), 16384)?;
                procfs::parse_stat(std::str::from_utf8(&bytes)?, pid)
            })();
            match row {
                Ok(row) => rows.push(row),
                Err(error) if missing(&error) => (),
                Err(error) => return Err(error),
            }
        }
        Ok(rows)
    }
    fn inspect(&mut self, identity: &Identity) -> Result<Option<Record>> {
        match record(identity.pid, self.uid) {
            Ok(row) => {
                ensure!(row.identity == *identity, "stale owned process identity");
                Ok(Some(row))
            }
            Err(error) if missing(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn pin(&mut self, expected: &Record) -> Result<Option<Pidfd>> {
        let Some(before) = self.inspect(&expected.identity)? else {
            return Ok(None);
        };
        ensure!(
            before.parent == expected.parent && before.session == expected.session,
            "process ancestry changed before pin"
        );
        // SAFETY: pidfd_open takes a validated positive PID and zero flags. A
        // successful syscall returns a new descriptor owned only by this value.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_pidfd_open,
                expected.identity.pid as libc::pid_t,
                0u32,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(None);
            }
            return Err(error.into());
        }
        let handle = Pidfd {
            fd: unsafe { OwnedFd::from_raw_fd(fd as i32) },
        };
        if self.exited(&handle)? {
            return Ok(None);
        }
        let after = self
            .inspect(&expected.identity)?
            .context("process disappeared while pinning")?;
        ensure!(
            before.identity == after.identity
                && before.parent == after.parent
                && before.session == after.session,
            "process identity changed while pinning"
        );
        Ok(Some(handle))
    }
    fn exited(&mut self, handle: &Pidfd) -> Result<bool> {
        let mut event = libc::pollfd {
            fd: handle.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            // SAFETY: event points to one initialized pollfd for the duration of poll.
            let result = unsafe { libc::poll(&mut event, 1, 0) };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.into());
            }
            ensure!(
                event.revents & (libc::POLLNVAL | libc::POLLERR) == 0,
                "owned pidfd failed"
            );
            return Ok(event.revents & libc::POLLIN != 0);
        }
    }
    fn signal(&mut self, handle: &Pidfd, signal: Signal) -> Result<()> {
        send(
            handle,
            match signal {
                Signal::Stop => libc::SIGSTOP,
                Signal::Continue => libc::SIGCONT,
                Signal::Kill => libc::SIGKILL,
            },
        )
    }
}
fn send(handle: &Pidfd, signal: i32) -> Result<()> {
    // SAFETY: the descriptor is held, siginfo is null, and flags are zero. This
    // never targets a recyclable numeric PID or process group.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            handle.fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error.into());
        }
    }
    Ok(())
}
fn valid_boot(boot: &str) -> bool {
    boot.len() == 36
        && boot.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
