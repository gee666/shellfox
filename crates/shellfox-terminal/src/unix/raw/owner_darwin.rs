//! macOS boundary: the controller leads its own session, and its original
//! unreaped Child reserves that session/group ID until cleanup is confirmed.
//! A target that calls setsid itself leaves the session and is not contained.
use crate::{
    launch_privilege::{NativeLaunchBinding, NativeLaunchEvidence},
    raw_process::RawCommand,
    unix::{
        darwin_io::{PtyIo, Stop},
        darwin_kqueue as kqueue, darwin_process,
    },
};
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::PermissionsExt, net::UnixStream, process::CommandExt},
    },
    path::Path,
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// The controller's end of the control socketpair.
pub(super) const CONTROL_FD: i32 = 3;
/// Backstop between member exit events while a session is torn down.
const TEARDOWN_RECHECK: Duration = Duration::from_millis(250);

pub(super) struct Launch {
    pub command: Command,
    pub owner: Owner,
    pub gate: Control,
}
/// Only the forked controller inherits its end; no name or path can reach it.
pub(super) struct Control {
    stream: Option<UnixStream>,
    controller: Option<OwnedFd>,
}
impl Control {
    pub fn release(
        &mut self,
        _pid: u32,
        payload: &[u8],
        before_go: impl FnOnce() -> Result<()>,
    ) -> Result<UnixStream> {
        // Our copy of the controller's end would hide its death from us.
        self.controller = None;
        let stream = self.stream.take().context("Raw control already released")?;
        crate::unix::handshake::release(stream, payload, before_go)
    }
}

pub(super) fn prepare(supervisor: &Path, command: &RawCommand, _bound: bool) -> Result<Launch> {
    let supervisor = crate::unix::private::helper_path(supervisor)?;
    let (stream, controller) = UnixStream::pair()?;
    let no_sigpipe: libc::c_int = 1;
    ensure!(
        unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&no_sigpipe as *const libc::c_int).cast(),
                std::mem::size_of_val(&no_sigpipe) as libc::socklen_t,
            )
        } == 0,
        "Raw control SIGPIPE setup failed"
    );
    // Above the stdio slots, so pre_exec can always move it to exactly fd 3.
    let fd = unsafe { libc::fcntl(controller.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 4) };
    ensure!(fd >= 0, "Raw control descriptor setup failed");
    let controller = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut builder = Command::new(supervisor);
    builder
        .arg(super::FLAG)
        .arg(std::process::id().to_string())
        .env_clear();
    unsafe {
        builder.pre_exec(move || {
            // dup2 leaves the new descriptor inheritable across exec.
            if libc::setsid() < 0 || libc::dup2(fd, CONTROL_FD) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let setid = std::fs::metadata(&command.executable)?.permissions().mode() & 0o6000 != 0;
    Ok(Launch {
        command: builder,
        owner: Owner {
            uid: unsafe { libc::getuid() },
            setid,
            sid: None,
            exited: false,
            pipes: Arc::default(),
            closed: Arc::default(),
        },
        gate: Control {
            stream: Some(stream),
            controller: Some(controller),
        },
    })
}

pub(super) struct Owner {
    uid: libc::uid_t,
    setid: bool,
    /// The controller's PID, which is also its session and group ID.
    sid: Option<libc::pid_t>,
    exited: bool,
    pipes: Arc<Stop>,
    closed: Arc<AtomicBool>,
}
impl Owner {
    /// The original Child stays unreaped until `remove`, which pins its PID.
    pub fn pin(&mut self, _child: &Child) -> Result<()> {
        Ok(())
    }
    pub fn attach(&mut self, child: &Child) -> Result<()> {
        let pid = child.id() as libc::pid_t;
        let info = darwin_process::process_info(pid, false)
            .context("read original raw controller credentials")?;
        ensure!(
            info.pbi_ppid == std::process::id()
                && info.pbi_pgid == pid as u32
                && unsafe { libc::getsid(pid) } == pid,
            "Raw controller does not lead its own session"
        );
        let gid = unsafe { libc::getgid() };
        ensure!(
            [info.pbi_ruid, info.pbi_uid, info.pbi_svuid] == [self.uid; 3]
                && [info.pbi_rgid, info.pbi_gid, info.pbi_svgid] == [gid; 3]
                && info.pbi_flags & darwin_process::PROC_FLAG_PSUGID == 0,
            "Raw controller credentials changed at launch"
        );
        self.sid = Some(pid);
        Ok(())
    }
    /// Never reaps: the session ID must stay reserved through cleanup.
    pub fn exited(&mut self, child: &mut Child) -> Result<bool> {
        if !self.exited {
            self.exited = kqueue::child_exited(child.id() as libc::pid_t)?;
        }
        Ok(self.exited)
    }
    pub fn reader(&self, pipe: impl Into<OwnedFd>) -> Result<Box<dyn Read + Send>> {
        let file = File::from(pipe.into());
        Ok(Box::new(PtyIo::new(file, Arc::clone(&self.pipes))?))
    }
    pub fn evidence(
        &self,
        binding: NativeLaunchBinding,
        _child: &Child,
    ) -> Result<NativeLaunchEvidence> {
        Ok(crate::launch_privilege::capture_darwin_raw(
            binding, self.uid, self.setid,
        ))
    }
    /// Wakes on the controller's exit (kqueue NOTE_EXIT), then on each exit
    /// of a remaining session member until the session is empty.
    pub fn watch(&self, _child: &Child, wake: crate::Wake) -> Result<()> {
        let sid = self.sid.context("Raw controller not attached")?;
        let (uid, closed) = (self.uid, Arc::clone(&self.closed));
        let queue = kqueue::queue()?;
        let watching = kqueue::watch_exit(&queue, sid)?;
        std::thread::Builder::new()
            .name("darwin-raw-exit".into())
            .spawn(move || {
                let recheck = Some(kqueue::EXIT_RECHECK);
                let mut waiting = watching;
                while waiting {
                    waiting = matches!(kqueue::next(&queue, recheck), Ok(None))
                        && !kqueue::child_exited(sid).unwrap_or(true);
                }
                wake();
                // There is no "session empty" event, so follow its members.
                while !closed.load(Ordering::Acquire) {
                    let members = darwin_process::session_member_pids(sid, uid);
                    if members.is_empty() {
                        break;
                    }
                    let watched = members
                        .into_iter()
                        .filter(|pid| kqueue::watch_exit(&queue, *pid).unwrap_or(false))
                        .count();
                    let recheck = if watched > 0 {
                        TEARDOWN_RECHECK
                    } else {
                        Duration::from_millis(5)
                    };
                    let _ = kqueue::next(&queue, Some(recheck));
                    wake();
                }
                wake();
            })
            .context("start raw exit watch")?;
        Ok(())
    }
    /// SIGKILL for the controller's group, then each other session member.
    pub fn terminate(&mut self) -> Result<()> {
        if let Some(sid) = self.sid {
            darwin_process::signal(-sid, libc::SIGKILL).context("kill raw process group")?;
            darwin_process::sweep_session(sid, self.uid, true);
        }
        Ok(())
    }
    pub fn empty(&mut self) -> Result<bool> {
        let empty = self
            .sid
            .is_none_or(|sid| darwin_process::session_member_pids(sid, self.uid).is_empty());
        if empty && self.exited {
            // Nothing in the session can write any more. A pipe still open now
            // is held by an escaped process, so its readers stop at idle.
            self.pipes.drain.store(true, Ordering::Release);
        }
        Ok(empty)
    }
    /// Only the trusted unreleased controller exists before GO.
    pub fn stop_unreleased(&mut self, child: &mut Child) -> Result<()> {
        child.kill().context("stop unreleased raw controller")
    }
    pub fn remove(&mut self, child: &mut Child) -> Result<()> {
        ensure!(
            self.exited && self.empty()?,
            "Raw controller session is still populated"
        );
        self.closed.store(true, Ordering::Release);
        child
            .try_wait()?
            .context("Raw controller exit not yet waitable")?;
        Ok(())
    }
    pub fn signal_controller(&self, child: &Child, signal: i32) -> Result<()> {
        // Unreaped, so this PID is still the original controller.
        ensure!(
            unsafe { libc::kill(child.id() as libc::pid_t, signal) } == 0,
            "signal original raw controller"
        );
        Ok(())
    }
}
