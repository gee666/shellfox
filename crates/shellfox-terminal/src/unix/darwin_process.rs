use anyhow::{ensure, Context, Result};
use portable_pty::{Child, ExitStatus};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};

type Pid = libc::pid_t;
const SIGNAL_GRACE: Duration = Duration::from_secs(3);
const SWEEP_INTERVAL: Duration = Duration::from_millis(40);
// Values from Darwin's <sys/proc_info.h>.
const PROC_UID_ONLY: u32 = 4;
const PROC_FLAG_PSUGID: u32 = 0x2000;
const MAX_PIDS: usize = 65_536;

#[link(name = "proc")]
extern "C" {}

pub(super) fn exit_queue() -> io::Result<File> {
    let fd = unsafe { libc::kqueue() };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let queue = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(queue)
}

pub(super) struct Leader {
    child: Box<dyn Child + Send + Sync>,
    queue: File,
    // The original child is not reaped before group cleanup. Its PID remains
    // reserved and cannot name a new process group while we send signals.
    sid: Pid,
    uid: libc::uid_t,
    watching: bool,
    exited: bool,
    stopping: Option<Instant>,
    next_sweep: Instant,
    status: Option<ExitStatus>,
}

impl Leader {
    pub fn new(child: Box<dyn Child + Send + Sync>, queue: File) -> Self {
        // The Unix portable-pty adapter always returns an original std Child.
        let sid = child.process_id().expect("Unix PTY child has a PID") as Pid;
        Self {
            child,
            queue,
            sid,
            uid: unsafe { libc::getuid() },
            watching: false,
            exited: false,
            stopping: None,
            next_sweep: Instant::now(),
            status: None,
        }
    }

    pub fn verify(&mut self) -> Result<libc::uid_t> {
        let info = process_info(self.sid, true).context("read original macOS PTY credentials")?;
        let gid = unsafe { libc::getgid() };
        ensure!(
            info.pbi_ppid == std::process::id() && info.pbi_pgid == self.sid as u32,
            "PTY did not start as the original process group leader"
        );
        ensure!(
            [info.pbi_ruid, info.pbi_uid, info.pbi_svuid] == [self.uid; 3]
                && [info.pbi_rgid, info.pbi_gid, info.pbi_svgid] == [gid; 3]
                && info.pbi_flags & PROC_FLAG_PSUGID == 0,
            "original macOS PTY credentials changed at launch"
        );
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        event.ident = self.sid as usize;
        event.filter = libc::EVFILT_PROC;
        event.flags = libc::EV_ADD | libc::EV_ONESHOT;
        event.fflags = libc::NOTE_EXIT;
        let result = unsafe {
            libc::kevent(
                self.queue.as_raw_fd(),
                &event,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if result == 0 {
            self.watching = true;
        } else {
            let error = io::Error::last_os_error();
            // A short-lived program may already have exited. Leave its status
            // waitable so cleanup still owns the same numeric group ID.
            if error.raw_os_error() != Some(libc::ESRCH) || !self.observe_exit()? {
                return Err(error).context("register macOS PTY child exit notification");
            }
        }
        Ok(self.uid)
    }

    fn observe_exit(&mut self) -> io::Result<bool> {
        if !self.exited {
            if self.watching {
                let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                let now = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                let count = unsafe {
                    libc::kevent(
                        self.queue.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        &mut event,
                        1,
                        &now,
                    )
                };
                match count {
                    1 => self.exited = event.fflags & libc::NOTE_EXIT != 0,
                    0 => (),
                    _ => {
                        let error = io::Error::last_os_error();
                        if error.kind() != io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
                }
            } else {
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                if unsafe {
                    libc::waitid(
                        libc::P_PID,
                        self.sid as libc::id_t,
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                } == 0
                {
                    self.exited = unsafe { info.si_pid() } == self.sid;
                } else {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
            }
        }
        Ok(self.exited)
    }

    pub fn begin(&mut self) -> Result<()> {
        if self.status.is_some() || self.stopping.is_some() {
            return Ok(());
        }
        // The PTY adapter calls setsid before exec. Never signal PID 0 or -1.
        ensure!(self.sid > 1, "invalid macOS PTY group ID");
        signal(-self.sid, libc::SIGHUP)?;
        signal(-self.sid, libc::SIGTERM)?;
        self.stopping = Some(Instant::now());
        self.next_sweep = Instant::now();
        Ok(())
    }

    pub fn pid(&self) -> u32 {
        self.sid as u32
    }
    pub fn poll(&mut self) -> Result<Option<ExitStatus>> {
        if let Some(status) = &self.status {
            return Ok(Some(status.clone()));
        }
        let exited = self.observe_exit()?;
        if exited {
            self.begin()?;
        }
        if let Some(started) = self.stopping {
            let now = Instant::now();
            if now >= self.next_sweep {
                self.next_sweep = now + SWEEP_INTERVAL;
                let force = now.duration_since(started) >= SIGNAL_GRACE;
                if force {
                    signal(-self.sid, libc::SIGKILL)?;
                }
                let remaining = self.sweep(force);
                if exited && !remaining {
                    self.status = self
                        .child
                        .try_wait()
                        .context("reap original macOS PTY child")?;
                }
            }
        }
        Ok(self.status.clone())
    }

    fn sweep(&self, force: bool) -> bool {
        let mut remaining = false;
        // A shell may create more process groups in the same session for job
        // control. Include reparented same-session members. An independent
        // setsid can escape this best-effort sweep; this is not containment.
        let mut pids = list_pids(|buf, size| {
            // The convenience API returns PID count; proc_listpids returns bytes.
            unsafe { libc::proc_listchildpids(self.sid, buf, size) }
                .saturating_mul(size_of::<Pid>() as libc::c_int)
        });
        pids.extend(list_pids(|buf, size| unsafe {
            libc::proc_listpids(PROC_UID_ONLY, self.uid, buf, size)
        }));
        pids.sort_unstable();
        pids.dedup();
        for pid in pids {
            if pid <= 1 || pid == self.sid || unsafe { libc::getsid(pid) } != self.sid {
                continue;
            }
            let Some(member) = process_info(pid, false) else {
                continue;
            };
            if [member.pbi_ruid, member.pbi_uid, member.pbi_svuid] != [self.uid; 3] {
                continue;
            }
            remaining = true;
            let same_member = |pid| {
                process_info(pid, false).is_some_and(|info| {
                    info.pbi_start_tvsec == member.pbi_start_tvsec
                        && info.pbi_start_tvusec == member.pbi_start_tvusec
                        && [info.pbi_ruid, info.pbi_uid, info.pbi_svuid] == [self.uid; 3]
                }) && unsafe { libc::getsid(pid) } == self.sid
            };
            // Recheck birth, session and credentials immediately before each
            // individual signal. Darwin has no pidfd for arbitrary members.
            if force && same_member(pid) {
                let _ = signal(pid, libc::SIGKILL);
            } else if !force && member.pbi_pgid != self.sid as u32 && same_member(pid) {
                let _ = signal(pid, libc::SIGHUP);
                if same_member(pid) {
                    let _ = signal(pid, libc::SIGTERM);
                }
            }
        }
        remaining
    }
}

fn process_info(pid: Pid, include_zombie: bool) -> Option<libc::proc_bsdinfo> {
    let mut result: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            u64::from(include_zombie),
            std::ptr::addr_of_mut!(result).cast(),
            size,
        )
    };
    (bytes == size && result.pbi_pid == pid as u32).then_some(result)
}

fn list_pids(list: impl Fn(*mut libc::c_void, libc::c_int) -> libc::c_int) -> Vec<Pid> {
    let mut pids = vec![0; 256];
    loop {
        let capacity = (pids.len() * size_of::<Pid>()) as libc::c_int;
        let bytes = list(pids.as_mut_ptr().cast(), capacity);
        if bytes < capacity || pids.len() == MAX_PIDS {
            pids.truncate((bytes.max(0) as usize) / size_of::<Pid>());
            return pids;
        }
        pids.resize(pids.len() * 2, 0);
    }
}

fn signal(target: Pid, signal: libc::c_int) -> io::Result<()> {
    if unsafe { libc::kill(target, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // Darwin can return EPERM for a group with only the unreaped zombie. A
    // live leader still has to exit; same-session members are checked below.
    if error.raw_os_error() == Some(libc::ESRCH)
        || target < -1 && error.raw_os_error() == Some(libc::EPERM)
    {
        Ok(())
    } else {
        Err(error)
    }
}
