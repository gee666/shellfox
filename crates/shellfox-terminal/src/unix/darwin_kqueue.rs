//! kqueue events for Darwin process ownership: exits, readable sockets, signals.
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::Duration;

/// Backstop recheck for an exit that raced its NOTE_EXIT registration.
pub(crate) const EXIT_RECHECK: Duration = Duration::from_secs(1);

pub(crate) fn queue() -> io::Result<File> {
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

pub(crate) fn add(
    queue: &File,
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
) -> io::Result<()> {
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    event.ident = ident;
    event.filter = filter;
    event.flags = libc::EV_ADD | flags;
    event.fflags = fflags;
    // SAFETY: one initialized change and no output events.
    let result = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// One-shot NOTE_EXIT for `pid`. False when the process has already exited.
pub(crate) fn watch_exit(queue: &File, pid: libc::pid_t) -> io::Result<bool> {
    match add(
        queue,
        pid as usize,
        libc::EVFILT_PROC,
        libc::EV_ONESHOT,
        libc::NOTE_EXIT,
    ) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        Err(error) => Err(error),
    }
}

/// The next event; `None` blocks. Returns `None` on timeout or interruption.
pub(crate) fn next(queue: &File, timeout: Option<Duration>) -> io::Result<Option<libc::kevent>> {
    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
    let limit = timeout.map(|timeout| libc::timespec {
        tv_sec: timeout.as_secs() as libc::time_t,
        tv_nsec: timeout.subsec_nanos().into(),
    });
    // SAFETY: one output event; the timeout lives across the call.
    let count = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            std::ptr::null(),
            0,
            &mut event,
            1,
            limit.as_ref().map_or(std::ptr::null(), |limit| limit),
        )
    };
    match count {
        1 => Ok(Some(event)),
        0 => Ok(None),
        _ => {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

/// Whether our child `pid` has exited, leaving it waitable (WNOWAIT).
pub(crate) fn child_exited(pid: libc::pid_t) -> io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: waitid only writes into `info`.
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } == 0
    {
        return Ok(unsafe { info.si_pid() } == pid);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
        Ok(false)
    } else {
        Err(error)
    }
}
