//! Dedicated original controller. Its child is waitable through actual exit;
//! all exec'd processes inherit the already-installed kernel cgroup boundary.
use anyhow::{ensure, Context, Result};
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
    // Open our exact already-assigned cgroup before creating any descendants.
    // On owner-channel loss this controller kills that boundary, including itself.
    let membership = std::fs::read_to_string("/proc/self/cgroup")?;
    let group = membership
        .strip_prefix("0::/")
        .and_then(|s| s.strip_suffix('\n'))
        .context("PTY cgroup-v2 membership missing")?;
    let leaf = group.rsplit('/').next().context("PTY cgroup missing")?;
    ensure!(
        leaf.starts_with("shellfox-")
            && leaf.ends_with(".scope")
            && !group.split('/').any(|s| s == ".."),
        "PTY not in owned cgroup"
    );
    let mut kill = std::fs::OpenOptions::new().write(true).open(
        PathBuf::from("/sys/fs/cgroup")
            .join(group)
            .join("cgroup.kill"),
    )?;
    // Cleanup must not log to the guest PTY: after hangup stderr can fail, and
    // before hangup it can block behind undrained guest output.
    let _ = run(&mut stream, &program, args);
    // A successful write kills this controller too. On failure stay alive with
    // this exact cgroup descriptor and retry, even if the host channel is gone.
    // Returning here would abandon descendants after a failed kill operation.
    loop {
        let _ = kill.write_all(b"1");
        std::thread::sleep(Duration::from_secs(1));
    }
}
fn run(
    stream: &mut UnixStream,
    program: &std::path::Path,
    args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<u32> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    ensure!(u32::from_le_bytes(length) == 0, "Unexpected PTY startup payload.");
    let mut command = Command::new(program);
    command.args(args);
    // An isolated tab gets no account session bus; a user tab keeps it.
    if std::env::var_os(ISOLATED).is_some() {
        command.env_remove(ISOLATED).env_remove("DBUS_SESSION_BUS_ADDRESS");
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
            // Same exit as owner-channel loss: entry retains and retries the
            // exact held cgroup.kill descriptor until termination succeeds.
            return Ok(0);
        }
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
