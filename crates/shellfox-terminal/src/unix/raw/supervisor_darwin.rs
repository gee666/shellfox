//! The macOS raw controller: a session leader that launches the target, reports
//! its exact wait status, then stops every remaining member of its session.
use super::{owner::CONTROL_FD, RawCommand, FLAG};
use crate::unix::{darwin_kqueue as kqueue, darwin_process};
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    time::Duration,
};

const CLOSE_SIGNALS: [i32; 3] = [libc::SIGHUP, libc::SIGTERM, libc::SIGINT];

pub(super) fn entry() -> Option<u32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(FLAG)) {
        return None;
    }
    // Never print parser/launch errors into the target's byte pipes.
    let result = enter(args);
    // Whatever happened, nothing launched from this session may outlive it.
    let me = std::process::id() as libc::pid_t;
    let uid = unsafe { libc::getuid() };
    while unsafe { libc::getsid(0) } == me && darwin_process::sweep_session(me, uid, true) {
        std::thread::sleep(Duration::from_millis(5));
    }
    Some(u32::from(result.is_err()))
}

fn enter(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    // Close requests become kqueue events, which are recorded even for ignored
    // signals, so no handler can interrupt the guest wait or the cleanup.
    let queue = kqueue::queue()?;
    for signal in CLOSE_SIGNALS {
        kqueue::add(&queue, signal as usize, libc::EVFILT_SIGNAL, 0, 0)?;
        ensure!(
            unsafe { libc::signal(signal, libc::SIG_IGN) } != libc::SIG_ERR,
            "Raw close signal setup failed"
        );
    }
    let parent: libc::pid_t = args
        .next()
        .context("Missing raw parent")?
        .to_str()
        .context("Invalid raw parent")?
        .parse()?;
    let me = std::process::id() as libc::pid_t;
    ensure!(
        args.next().is_none()
            && unsafe { libc::getppid() } == parent
            && unsafe { libc::getsid(0) } == me
            && unsafe { libc::getpgrp() } == me
            && unsafe { libc::getuid() } == unsafe { libc::geteuid() },
        "Raw controller identity changed"
    );
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::fstat(CONTROL_FD, &mut stat) } == 0
            && stat.st_mode & libc::S_IFMT == libc::S_IFSOCK
            && unsafe { libc::fcntl(CONTROL_FD, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "Raw control socket missing"
    );
    // The target must not inherit the control socket: FD_CLOEXEC is set above.
    let mut stream = unsafe { UnixStream::from_raw_fd(CONTROL_FD) };
    let command = super::controller::released_command(&mut stream)?;
    ensure!(
        !close_requested(&queue)?,
        "Raw close requested before target exec"
    );
    run(&mut stream, &queue, &command)
}

fn run(stream: &mut UnixStream, queue: &File, command: &RawCommand) -> Result<()> {
    let mut child = super::exec::spawn(command).context("Raw target exec failed")?;
    let pid = child.id() as libc::pid_t;
    // Only the target/its descendants may retain pipe ends. Keeping our own
    // stdin reader would hide EPIPE, and stdout writers would delay real EOF.
    for fd in [0, 1, 2] {
        unsafe {
            libc::close(fd);
        }
    }
    let watching = kqueue::watch_exit(queue, pid)?;
    stream.write_all(b"START\n")?;
    stream.set_nonblocking(true)?;
    // Any control byte (K), EOF from a lost owner, or an error closes.
    kqueue::add(
        queue,
        stream.as_raw_fd() as usize,
        libc::EVFILT_READ,
        libc::EV_ONESHOT,
        0,
    )?;
    let mut stopping = false;
    let mut exited = !watching;
    loop {
        if exited {
            // The original Child's own wait status, never a PID lookup.
            let status = child.wait()?;
            stream.set_nonblocking(false)?;
            stream.write_all(&super::controller::exit_report(status))?;
            return Ok(());
        }
        match kqueue::next(queue, Some(kqueue::EXIT_RECHECK))? {
            Some(event) if event.filter == libc::EVFILT_PROC => exited = true,
            Some(_) if !stopping => {
                // Unreaped, so this PID is still the original guest.
                child
                    .kill()
                    .context("Raw original guest termination failed")?;
                stopping = true;
            }
            Some(_) => (),
            None => exited = kqueue::child_exited(pid)?,
        }
    }
}

fn close_requested(queue: &File) -> Result<bool> {
    Ok(kqueue::next(queue, Some(Duration::ZERO))?
        .is_some_and(|event| event.filter == libc::EVFILT_SIGNAL))
}
