//! Containment without a cgroup: one controller's session, plus orphans
//! reparented to that controller as their subreaper. A process that starts
//! its own session while its parent still lives leaves this boundary, and so
//! does a setuid member this account may not signal.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

/// Exit events watched at once; the recheck covers the rest.
const MAX_WATCHED: usize = 256;

/// Named by its leader's PID. Callers keep the leader unreaped until the
/// session is empty, so the number can't name another session meanwhile.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Session {
    sid: u32,
}
impl Session {
    pub fn new(sid: u32) -> Result<Self> {
        ensure!(sid > 1 && sid <= i32::MAX as u32, "invalid session leader");
        same_pid_namespace()?;
        Ok(Self { sid })
    }
    /// The calling process, which must lead its own session.
    pub fn own() -> Result<Self> {
        let me = std::process::id();
        ensure!(
            unsafe { libc::getsid(0) } == me as libc::pid_t,
            "controller does not lead its own session"
        );
        Self::new(me)
    }
    /// SIGKILL to every member. True while a member we may signal remains.
    pub fn kill(&self) -> Result<bool> {
        self.signal(libc::SIGKILL)
    }
    pub fn populated(&self) -> Result<bool> {
        self.signal(0)
    }
    /// The leader and its members, for read-only inspection.
    pub fn pids(&self) -> Result<BTreeSet<u32>> {
        let mut pids = BTreeSet::from([self.sid]);
        self.each(|pid, pidfd| {
            if send(&pidfd, 0)? {
                pids.insert(pid);
            }
            Ok(())
        })?;
        Ok(pids)
    }
    /// Waits for a member exit, at most `recheck`: a new member has no exit
    /// event yet. False once no member we may signal remains.
    pub fn wait(&self, recheck: Duration) -> Result<bool> {
        let mut watched = Vec::new();
        let mut remaining = false;
        self.each(|_, pidfd| {
            if send(&pidfd, 0)? {
                remaining = true;
                if watched.len() < MAX_WATCHED {
                    watched.push(pidfd);
                }
            }
            Ok(())
        })?;
        if !remaining {
            return Ok(false);
        }
        let mut polls: Vec<_> = watched
            .iter()
            .map(|pidfd| libc::pollfd {
                fd: pidfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let timeout = i32::try_from(recheck.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: every pollfd names a pidfd held in `watched` for this call.
        unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, timeout) };
        Ok(true)
    }
    fn signal(&self, signal: i32) -> Result<bool> {
        let mut remaining = false;
        self.each(|_, pidfd| {
            remaining |= send(&pidfd, signal)?;
            Ok(())
        })?;
        Ok(remaining)
    }
    /// Live processes in the session, or direct children of its leader, each
    /// pinned by a pidfd before it is trusted. One descriptor at a time, so a
    /// tree larger than the descriptor limit is still swept.
    fn each(&self, mut visit: impl FnMut(u32, OwnedFd) -> Result<()>) -> Result<()> {
        for entry in std::fs::read_dir("/proc")? {
            let Some(pid) = entry?
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if pid != self.sid {
                if let Some(pidfd) = self.pin(pid)? {
                    visit(pid, pidfd)?;
                }
            }
        }
        Ok(())
    }
    fn pin(&self, pid: u32) -> Result<Option<OwnedFd>> {
        // A /proc/<pid> descriptor keeps naming its original process: once
        // that exits, reads through it fail even if the PID is reused.
        let Ok(directory) = File::open(format!("/proc/{pid}")) else {
            return Ok(None);
        };
        if !self.belongs(&directory, pid) {
            return Ok(None);
        }
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::ESRCH) => Ok(None),
                _ => Err(error.into()),
            };
        }
        let pidfd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        // Still readable, so the pidfd pins the same process we matched.
        Ok(self.belongs(&directory, pid).then_some(pidfd))
    }
    fn belongs(&self, directory: &File, pid: u32) -> bool {
        let mut text = String::new();
        let read = super::super::cgroup::open_at(directory, c"stat", libc::O_RDONLY)
            .and_then(|file| Ok(file.take(16384).read_to_string(&mut text)?));
        read.is_ok()
            && super::stat::parse_stat(&text, pid)
                .is_ok_and(|row| !row.dead() && (row.session == self.sid || row.parent == self.sid))
    }
}

/// /proc must show our own PID namespace: its numbers are matched, then
/// pinned through pidfd_open, which uses ours. A foreign /proc would pin the
/// wrong processes.
pub(crate) fn same_pid_namespace() -> Result<()> {
    let me = std::fs::read_link("/proc/self").context("read /proc/self")?;
    ensure!(
        me.to_str() == Some(std::process::id().to_string().as_str()),
        "/proc belongs to another PID namespace"
    );
    Ok(())
}

/// True if the process may still be signalled; false once it is gone or
/// belongs to an account we can't signal.
fn send(pidfd: &OwnedFd, signal: i32) -> Result<bool> {
    // SAFETY: a held pidfd, null siginfo and zero flags.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH | libc::EPERM) => Ok(false),
        _ => Err(error.into()),
    }
}

/// True once the pidfd's process has exited, reaped or not.
pub(crate) fn exited(pidfd: &File) -> Result<bool> {
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd for a held descriptor, no wait.
    let ready = unsafe { libc::poll(&mut poll, 1, 0) };
    ensure!(
        ready >= 0,
        "poll pidfd: {}",
        std::io::Error::last_os_error()
    );
    Ok(ready == 1 && poll.revents & libc::POLLIN != 0)
}

/// Blocks until the pidfd's process exits.
pub(crate) fn wait_exit(pidfd: &File) {
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd for a held descriptor.
    while unsafe { libc::poll(&mut poll, 1, -1) } < 0
        && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
    {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    #[test]
    fn a_session_sweep_kills_members_that_ignore_hangup() {
        // A leader in its own session with a child that ignores SIGHUP.
        let mut leader = Command::new("/bin/sh");
        leader.args(["-c", "trap '' HUP; sleep 30 & wait"]);
        unsafe {
            leader.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut leader = leader.spawn().unwrap();
        let session = Session::new(leader.id()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while session.pids().unwrap().len() < 2 {
            assert!(std::time::Instant::now() < deadline, "member never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(session.populated().unwrap());
        while session.kill().unwrap() {
            assert!(std::time::Instant::now() < deadline, "member survived");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!session.wait(Duration::ZERO).unwrap());
        let _ = leader.kill();
        leader.wait().unwrap();
    }

    #[test]
    fn init_and_invalid_ids_are_never_sessions() {
        assert!(Session::new(0).is_err());
        assert!(Session::new(1).is_err());
        assert!(Session::new(u32::MAX).is_err());
    }
}
