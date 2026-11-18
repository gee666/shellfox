#[cfg(unix)]
fn fixture() -> anyhow::Result<bool> {
    use std::os::unix::net::UnixListener;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::time::Duration;
    let args: Vec<_> = std::env::args_os().collect();
    let Some(mode) = args.get(1).and_then(|s| s.to_str()) else {
        return Ok(false);
    };
    if mode == "--fixture-leaf" {
        let endpoint = PathBuf::from(
            args.get(2)
                .ok_or_else(|| anyhow::anyhow!("leaf socket missing"))?,
        );
        // SAFETY: fixture-only process, started before threads. Ignore terminal
        // hangup so closing the PTY alone cannot fake descendant cleanup.
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
        let listener = UnixListener::bind(endpoint)?;
        loop {
            let _ = listener.accept()?;
        }
    }
    if mode == "--fixture-detach" {
        let endpoint = args
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("leaf socket missing"))?;
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.arg("--fixture-leaf").arg(endpoint);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let _child = command.spawn()?;
        // Exit the intermediate parent. The detached leaf is reparented away
        // from both the logical terminal root and its session/process group.
        std::process::exit(0);
    }
    if matches!(
        mode,
        "--fixture-root" | "--fixture-double-root" | "--fixture-crash-controller"
    ) {
        anyhow::ensure!(
            !std::env::vars_os()
                .any(|(key, _)| key.as_encoded_bytes().starts_with(b"SHELLFOX_SUPERVISOR_")),
            "supervisor token leaked into the interactive command"
        );
        let endpoint = PathBuf::from(
            args.get(2)
                .ok_or_else(|| anyhow::anyhow!("leaf socket missing"))?,
        );
        let exit_file = PathBuf::from(
            args.get(3)
                .ok_or_else(|| anyhow::anyhow!("exit file missing"))?,
        );
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .arg(if mode == "--fixture-double-root" {
                "--fixture-detach"
            } else {
                "--fixture-leaf"
            })
            .arg(endpoint);
        // SAFETY: only async-signal-safe POSIX operations run between fork/exec.
        unsafe {
            command.pre_exec(|| {
                #[cfg(target_os = "linux")]
                let result = libc::setsid();
                #[cfg(not(target_os = "linux"))]
                let result = libc::setpgid(0, 0);
                if result < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        if mode == "--fixture-double-root" {
            anyhow::ensure!(child.wait()?.success(), "intermediate fixture failed");
        }
        while !exit_file.exists() {
            if mode != "--fixture-double-root" {
                anyhow::ensure!(
                    child.try_wait()?.is_none(),
                    "owned leaf exited before cleanup"
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if mode == "--fixture-crash-controller" {
            let parent = unsafe { libc::getppid() };
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, parent, 0) };
            anyhow::ensure!(fd >= 0, "pin fixture controller");
            anyhow::ensure!(
                unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        fd,
                        libc::SIGKILL,
                        std::ptr::null::<libc::siginfo_t>(),
                        0,
                    )
                } == 0,
                "crash original fixture controller"
            );
            unsafe {
                libc::close(i32::try_from(fd)?);
            }
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        // Intentionally leave the leaf running to exercise parent-root exit.
        std::process::exit(7);
    }
    Ok(false)
}

fn main() {
    match shellfox_terminal::supervisor_entry() {
        Ok(Some(code)) => std::process::exit(code as i32),
        Ok(None) => (),
        Err(error) => {
            eprintln!("production Unix supervisor failed: {error:#}");
            std::process::exit(72);
        }
    }
    #[cfg(unix)]
    match fixture() {
        Ok(true) => return,
        Ok(false) => (),
        Err(error) => {
            eprintln!("Unix fixture failed: {error:#}");
            std::process::exit(71);
        }
    }
    #[cfg(target_os = "linux")]
    match shellfox_terminal::unix::linux::gate::waiting_entry() {
        Ok(Some(code)) => std::process::exit(code as i32),
        Ok(None) => std::process::exit(64),
        Err(error) => {
            eprintln!("Unix waiting supervisor refused startup: {error:#}");
            std::process::exit(70);
        }
    }
    #[cfg(not(target_os = "linux"))]
    std::process::exit(64);
}
