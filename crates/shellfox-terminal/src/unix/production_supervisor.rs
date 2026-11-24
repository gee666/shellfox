//! Dedicated original controller. Its child is waitable through actual exit;
//! all exec'd processes inherit the already-installed kernel cgroup boundary,
//! or this controller's session when there is no cgroup.
use super::{linux::sweep::Session, Containment};
use anyhow::{ensure, Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::{
    net::{SocketAddr, UnixStream},
    process::{CommandExt, ExitStatusExt},
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set by the host for an isolated tab; never reaches the shell.
pub(super) const ISOLATED: &str = "SHELLFOX_PTY_ISOLATED";
pub(super) static CLOSE: AtomicBool = AtomicBool::new(false);
extern "C" fn request_close(_: libc::c_int) {
    // AtomicBool is lock-free on the supported Linux targets. No I/O, allocation
    // or lock may run here; the controller loop owns cgroup termination.
    CLOSE.store(true, Ordering::Relaxed);
}
pub(super) fn install_close_handlers() -> Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = request_close as *const () as libc::sighandler_t;
    action.sa_flags = 0;
    ensure!(
        unsafe { libc::sigemptyset(&mut action.sa_mask) } == 0,
        "PTY close signal mask unavailable"
    );
    for signal in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
        ensure!(
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0,
            "PTY controller close handler unavailable"
        );
    }
    Ok(())
}

/// How a closing controller ends everything it started. Opened after GO and
/// before the guest starts, so a failure here launches nothing.
pub(super) enum Teardown {
    /// Our exact, already-assigned cgroup's kill file.
    Cgroup(File),
    /// Our own session, plus orphans reparented to us as their subreaper.
    Session(Session),
}
impl Teardown {
    pub fn open(containment: Containment) -> Result<Self> {
        if containment == Containment::Session {
            let session = Session::own()?;
            ensure!(
                unsafe {
                    libc::prctl(
                        libc::PR_SET_CHILD_SUBREAPER,
                        1 as libc::c_ulong,
                        0 as libc::c_ulong,
                        0 as libc::c_ulong,
                        0 as libc::c_ulong,
                    )
                } == 0,
                "controller subreaper unavailable"
            );
            return Ok(Self::Session(session));
        }
        let membership = std::fs::read_to_string("/proc/self/cgroup")?;
        let group = membership
            .strip_prefix("0::/")
            .and_then(|s| s.strip_suffix('\n'))
            .context("cgroup-v2 membership missing")?;
        let leaf = group.rsplit('/').next().context("cgroup missing")?;
        ensure!(
            leaf.starts_with("shellfox-")
                && leaf.ends_with(".scope")
                && !group.split('/').any(|s| s == ".."),
            "not in an owned cgroup"
        );
        let kill = std::fs::OpenOptions::new().write(true).open(
            std::path::Path::new("/sys/fs/cgroup")
                .join(group)
                .join("cgroup.kill"),
        )?;
        Ok(Self::Cgroup(kill))
    }
    /// Reaps orphans that exited while the guest runs, never the guest.
    pub fn reap_orphans(&self, guest: u32) {
        if matches!(self, Self::Cgroup(_)) {
            return;
        }
        for _ in 0..64 {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: WNOWAIT only reports; the guest stays waitable.
            let peeked = unsafe {
                libc::waitid(
                    libc::P_ALL,
                    0,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            let pid = unsafe { info.si_pid() };
            if peeked != 0 || pid <= 0 || pid as u32 == guest {
                return;
            }
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        }
    }
    /// Kills everything this controller started, including itself. On
    /// failure it stays alive and retries: returning would abandon the tree.
    pub fn finish(self) -> ! {
        match self {
            Self::Cgroup(mut kill) => loop {
                let _ = kill.write_all(b"1");
                std::thread::sleep(Duration::from_secs(1));
            },
            Self::Session(session) => {
                loop {
                    while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
                    if matches!(session.kill(), Ok(false)) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                // The same end as a cgroup kill, so the host sees one outcome.
                loop {
                    unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }
}

pub(super) fn apply_privilege_policy(policy: &std::ffi::OsStr) -> Result<()> {
    match policy.to_str() {
        Some("local") => (),
        Some("non-elevating") => ensure!(
            unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } == 0,
            "cannot establish non-elevating POSIX launch"
        ),
        _ => anyhow::bail!("invalid POSIX launch policy"),
    }
    Ok(())
}

pub(crate) fn entry() -> Result<Option<u32>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(super::startup::FLAG)) {
        return Ok(None);
    }
    // PTY hangup can reach this session leader independently of the owner
    // socket. Install handlers before READY, before any guest can be spawned.
    install_close_handlers()?;
    let name = args.next().context("missing PTY gate")?;
    let name = name.to_str().context("invalid PTY gate")?;
    ensure!(
        name.strip_prefix("shellfox-")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        "invalid PTY gate"
    );
    let parent: u32 = args
        .next()
        .context("missing PTY parent")?
        .to_str()
        .context("invalid PTY parent")?
        .parse()?;
    let policy = args.next().context("missing POSIX launch policy")?;
    apply_privilege_policy(&policy)?;
    let containment = Containment::parse(args.next().as_deref())?;
    let program = PathBuf::from(args.next().context("missing PTY program")?);
    ensure!(
        program.is_absolute()
            && unsafe { libc::getppid() } == parent as libc::pid_t
            && unsafe { libc::getsid(0) } == std::process::id() as libc::pid_t
            && unsafe { libc::getuid() } == unsafe { libc::geteuid() },
        "PTY supervisor identity changed"
    );
    let mut stream = UnixStream::connect_addr(&SocketAddr::from_abstract_name(name)?)?;
    let credentials = super::startup::peer(&stream)?;
    ensure!(
        credentials.pid == parent as libc::pid_t && credentials.uid == unsafe { libc::getuid() },
        "PTY owner changed"
    );
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(b"READY\n")?;
    let mut go = [0; 3];
    stream.read_exact(&mut go)?;
    ensure!(&go == b"GO\n", "PTY not released");
    // On owner-channel loss this controller ends its boundary, including itself.
    let teardown = Teardown::open(containment)?;
    // Cleanup must not log to the guest PTY: after hangup stderr can fail, and
    // before hangup it can block behind undrained guest output.
    let _ = run(&mut stream, &teardown, &program, args);
    teardown.finish()
}
fn run(
    stream: &mut UnixStream,
    teardown: &Teardown,
    program: &std::path::Path,
    args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<u32> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    ensure!(
        u32::from_le_bytes(length) == 0,
        "Unexpected PTY startup payload."
    );
    let mut command = Command::new(program);
    command.args(args);
    // An isolated tab gets no account session bus; a user tab keeps it.
    if std::env::var_os(ISOLATED).is_some() {
        command
            .env_remove(ISOLATED)
            .env_remove("DBUS_SESSION_BUS_ADDRESS");
    }
    ensure!(
        unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN) } != libc::SIG_ERR,
        "PTY foreground setup failed"
    );
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // SIGTTOU is still ignored here. Reset it only after taking the
            // foreground, otherwise tcsetpgrp stops this child before exec.
            if libc::tcsetpgrp(0, libc::getpid()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for signal in [
                libc::SIGHUP,
                libc::SIGTERM,
                libc::SIGINT,
                libc::SIGTTOU,
                libc::SIGTTIN,
                libc::SIGTSTP,
            ] {
                if libc::signal(signal, libc::SIG_DFL) == libc::SIG_ERR {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    ensure!(
        !CLOSE.load(Ordering::Relaxed),
        "PTY controller close requested before exec"
    );
    let mut child = command.spawn().context("exec owned PTY program")?;
    // From here even IPC failure goes through entry's cgroup.kill.
    stream.write_all(b"START\n")?;
    stream.set_nonblocking(true)?;
    let mut sent_exit = false;
    loop {
        if CLOSE.load(Ordering::Relaxed) {
            // Same exit as owner-channel loss: entry's teardown.
            return Ok(0);
        }
        teardown.reap_orphans(child.id());
        if !sent_exit {
            if let Some(status) = child.try_wait()? {
                let code = status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(1));
                stream.write_all(&code.to_le_bytes())?;
                sent_exit = true;
            }
        }
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(_) => return Ok(0),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e.into()),
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
