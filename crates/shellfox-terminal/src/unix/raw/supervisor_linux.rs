use super::{RawCommand, FLAG};
use crate::unix::{
    production_supervisor::{apply_privilege_policy, install_close_handlers, Teardown, CLOSE},
    startup::peer,
    Containment,
};
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        linux::net::SocketAddrExt,
        unix::net::{SocketAddr, UnixStream},
    },
    sync::atomic::Ordering,
    time::Duration,
};
const WAIT: Duration = Duration::from_millis(50);
pub(super) fn entry() -> Option<u32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(FLAG)) {
        return None;
    }
    // Never print parser/launch errors into the target's byte pipes.
    Some(enter(args).unwrap_or(1))
}
fn enter(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<u32> {
    install_close_handlers()?;
    ensure!(
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) } != libc::SIG_ERR,
        "Raw guest wait capability unavailable"
    );
    let name = args.next().context("Missing raw gate")?;
    let name = name.to_str().context("Invalid raw gate")?;
    ensure!(
        name.strip_prefix("shellfox-")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        "Invalid raw gate"
    );
    let parent: u32 = args
        .next()
        .context("Missing raw parent")?
        .to_str()
        .context("Invalid raw parent")?
        .parse()?;
    let policy = args.next().context("Missing raw launch policy")?;
    apply_privilege_policy(&policy)?;
    let containment = Containment::parse(args.next().as_deref())?;
    ensure!(
        args.next().is_none()
            && unsafe { libc::getppid() } == parent as libc::pid_t
            && unsafe { libc::getsid(0) } == std::process::id() as libc::pid_t
            && unsafe { libc::getuid() } == unsafe { libc::geteuid() },
        "Raw controller identity changed"
    );
    let mut stream = UnixStream::connect_addr(&SocketAddr::from_abstract_name(name)?)?;
    let credentials = peer(&stream)?;
    ensure!(
        credentials.pid == parent as libc::pid_t && credentials.uid == unsafe { libc::getuid() },
        "Raw parent peer changed"
    );
    let command = super::controller::released_command(&mut stream)?;
    let teardown = Teardown::open(containment)?;
    let _ = run(&mut stream, &teardown, &command);
    // Same live retention as the PTY controller. Neither diagnostics nor
    // signal handlers may bypass or block this teardown and its retries.
    teardown.finish()
}
fn run(stream: &mut UnixStream, teardown: &Teardown, command: &RawCommand) -> Result<()> {
    ensure!(
        !CLOSE.load(Ordering::Relaxed),
        "Raw close requested before target exec"
    );
    let mut child = super::exec::spawn(command).context("Raw target exec failed")?;
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
    ensure!(fd >= 0, "Raw original guest pin failed");
    let guest = unsafe { File::from_raw_fd(i32::try_from(fd)?) };
    // Only the target/its descendants may retain pipe ends. Keeping our own
    // stdin reader would hide EPIPE, and stdout writers would delay real EOF.
    for fd in [0, 1, 2] {
        unsafe {
            libc::close(fd);
        }
    }
    stream.write_all(b"START\n")?;
    stream.set_nonblocking(true)?;
    let mut stopping = false;
    let mut signaled = false;
    loop {
        if let Some(status) = child.try_wait()? {
            stream.write_all(&super::controller::exit_report(status))?;
            return Ok(());
        }
        teardown.reap_orphans(Some(child.id()));
        stopping |= CLOSE.load(Ordering::Relaxed);
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(_) => stopping = true, // Explicit K, EOF, or malformed control all close.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => stopping = true,
        }
        if stopping && !signaled {
            let ok = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    guest.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            ensure!(
                ok == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
                "Raw original guest termination failed"
            );
            signaled = true;
        }
        wait(stream, &guest, !signaled);
    }
}
/// Sleeps until the guest exits or, while `control`, a stop request arrives.
/// A stop must kill at once: a guest that the host drains after asking can
/// otherwise finish by itself and report its own exit instead of SIGKILL.
/// The timeout bounds a close signal that lands just before the poll, and
/// orphan reaping.
fn wait(stream: &UnixStream, guest: &File, control: bool) {
    let mut polls = [
        libc::pollfd {
            fd: guest.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let count = if control { 2 } else { 1 };
    // SAFETY: `count` pollfds naming descriptors held for this call. EINTR
    // from a close signal just returns early.
    unsafe { libc::poll(polls.as_mut_ptr(), count, WAIT.as_millis() as i32) };
}
