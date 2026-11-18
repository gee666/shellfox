//! Persistent subreaper. Keeping this exact session leader alive prevents a
//! shell exit or double-fork/setsid from losing descendant ownership.
use super::{pidfd::LinuxKernel, procfs};
use crate::unix::tree::{Kernel, Tree};
use anyhow::{ensure, Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static CLOSE: AtomicBool = AtomicBool::new(false);
extern "C" fn request_close(_: libc::c_int) {
    CLOSE.store(true, Ordering::Relaxed);
}

pub(super) fn run(
    stream: &mut UnixStream,
    command: &std::path::Path,
    args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<u32> {
    // SAFETY: this dedicated supervisor runs before threads. Child subreaping
    // persists until its own verified cleanup; no other host sessions use it.
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
        "Linux subreaper unavailable"
    );
    for signal in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
        ensure!(
            unsafe { libc::signal(signal, request_close as *const () as libc::sighandler_t) }
                != libc::SIG_ERR,
            "supervisor close signal handler unavailable"
        );
    }
    ensure!(
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) } != libc::SIG_ERR,
        "waitable direct-child status unavailable"
    );
    // Ignore TTOU only in this supervisor. The shell resets it before exec.
    ensure!(
        unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN) } != libc::SIG_ERR,
        "supervisor foreground control unavailable"
    );
    let mut kernel = LinuxKernel::preflight()?;
    let own = procfs::record(std::process::id(), kernel.uid)?;
    let handle = kernel
        .pin(&own)?
        .context("supervisor self identity missing")?;
    let mut tree = Tree::supervisor_self(own.identity, handle)?;
    stream.set_nonblocking(true)?;
    let mut command_builder = Command::new(command);
    command_builder.args(args);
    // SAFETY: only async-signal-safe operations run between fork and exec.
    unsafe {
        command_builder.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
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
    // std's exec-error pipe confirms exec success before START. The direct
    // child is retained unreaped until its actual wait event is observed.
    let mut shell = match command_builder.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = stream.write_all(b"ERROR\n");
            return Err(error.into());
        }
    };
    let startup: Result<()> = (|| {
        let row = procfs::record(shell.id(), kernel.uid)?;
        ensure!(
            row.parent == std::process::id() && row.session == std::process::id(),
            "shell ancestry changed"
        );
        tree.track(&mut kernel)?;
        ensure!(
            unsafe { libc::tcsetpgrp(libc::STDIN_FILENO, shell.id() as libc::pid_t) } == 0,
            "shell foreground group unavailable"
        );
        stream.write_all(format!("START\t{}\t{}\n", shell.id(), row.identity.ticks).as_bytes())?;
        Ok(())
    })();
    let mut shell_status = None;
    if startup.is_err() {
        CLOSE.store(true, Ordering::Relaxed);
    }
    loop {
        if shell_status.is_none() {
            match shell.try_wait() {
                Ok(status) => shell_status = status,
                Err(_) => CLOSE.store(true, Ordering::Relaxed),
            }
        }
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(_) => CLOSE.store(true, Ordering::Relaxed), // EOF or unexpected command.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => CLOSE.store(true, Ordering::Relaxed),
        }
        if shell_status.is_some() || CLOSE.load(Ordering::Relaxed) {
            if tree.close(&mut kernel, Duration::from_secs(3)).is_ok() {
                // No descendant can execute now. Reap only after verified close.
                if shell_status.is_none() {
                    shell_status = Some(shell.wait()?);
                }
                loop {
                    let mut status = 0;
                    let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
                    if pid <= 0 {
                        break;
                    }
                }
                let status = shell_status.context("direct shell exit missing")?;
                return Ok(status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
                    as u32);
            }
            // Freeze failure restored survivors. Keep the subreaper and every
            // pidfd alive for retry, including after shell/host exit.
        } else {
            // Subreaping closes the detached-orphan race even between monitor
            // ticks. Periodic exact pins also preserve ordinary live ancestry.
            let _ = tree.track(&mut kernel);
            if let Ok(rows) = kernel.snapshot() {
                for row in rows.iter().filter(|r| {
                    r.parent == std::process::id() && r.identity.pid != shell.id() && r.dead()
                }) {
                    // Only already-dead unreaped direct orphan children. Never
                    // reap the shell here or infer ownership from a cached PID.
                    let mut status = 0;
                    unsafe {
                        libc::waitpid(row.identity.pid as libc::pid_t, &mut status, libc::WNOHANG);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
