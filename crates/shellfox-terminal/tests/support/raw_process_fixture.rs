use std::{
    io::{self, IsTerminal, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

fn main() -> anyhow::Result<()> {
    if let Some(code) = shellfox_terminal::supervisor_entry()? {
        std::process::exit(code as i32);
    }
    if let Some(code) = shellfox_terminal::raw_process::supervisor_entry() {
        std::process::exit(code as i32);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        #[cfg(unix)]
        Some("unix-leaf") => {
            use std::os::unix::net::UnixListener;
            unsafe {
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
            }
            let listener = UnixListener::bind(&args[1])?;
            loop {
                let _ = listener.accept()?;
            }
        }
        #[cfg(unix)]
        Some("unix-branch") => {
            use std::os::unix::process::CommandExt;
            let mut leaf = Command::new(std::env::current_exe()?);
            leaf.args(["unix-leaf", &args[1]]);
            // Linux contains a setsid escape in the cgroup. A macOS session
            // cannot, so there the leaf only leaves its parent's process group.
            unsafe {
                leaf.pre_exec(|| {
                    let left = if cfg!(target_os = "linux") {
                        libc::setsid()
                    } else {
                        libc::setpgid(0, 0)
                    };
                    if left < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let _leaf = leaf.spawn()?;
            io::stdout().write_all(b"RAW_ROOT_READY\n")?;
            io::stdout().flush()?;
            if args.get(2).is_some_and(|s| s == "natural") {
                io::copy(&mut io::stdin(), &mut io::sink())?;
                io::stdout().write_all(b"RAW_ROOT_FINAL\0\xff")?;
                io::stdout().flush()?;
                std::process::exit(7);
            }
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        #[cfg(unix)]
        Some("unix-self-kill") => {
            unsafe {
                libc::raise(libc::SIGKILL);
            }
            unreachable!();
        }
        #[cfg(unix)]
        Some("unix-close-input") => {
            unsafe {
                libc::close(0);
            }
            println!("RAW_INPUT_CLOSED");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        Some("echo") => {
            io::copy(&mut io::stdin(), &mut io::stdout())?;
            io::stderr().write_all(&[0, 255, 10])?;
            std::process::exit(7);
        }
        Some("prefill-echo") => {
            let block = [123; 32768];
            for _ in 0..64 {
                io::stdout().write_all(&block)?;
            }
            if let Some(release) = args.get(1) {
                let deadline = std::time::Instant::now() + Duration::from_secs(15);
                while !std::path::Path::new(release).is_file() {
                    anyhow::ensure!(
                        std::time::Instant::now() < deadline,
                        "stdin fixture was not released"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            let max = shellfox_terminal::raw_process::INPUT_BYTES
                * if args.get(1).is_some() { 16 } else { 1 };
            let mut input = Vec::new();
            io::stdin().take(max as u64 + 1).read_to_end(&mut input)?;
            anyhow::ensure!(input.len() <= max, "Owned fixture stdin exceeds its bound.");
            io::stdout().write_all(&input)?;
            io::stderr().write_all(&[0, 255, 10])?;
            std::process::exit(7);
        }
        Some("exit-code") => {
            let code: u32 = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Missing owned exit code."))?
                .parse()?;
            std::process::exit(code as i32);
        }
        Some("describe") => {
            let env: std::collections::BTreeMap<_, _> = std::env::vars().collect();
            let value = serde_json::json!({"args":&args[1..],"cwd":std::env::current_dir()?.to_str(),"env":env,
                "stdinTty":io::stdin().is_terminal(),"stdoutTty":io::stdout().is_terminal(),"stderrTty":io::stderr().is_terminal()});
            io::stdout().write_all(&serde_json::to_vec(&value)?)?;
        }
        Some("flood") | Some("flood-branch") => {
            if args[0] == "flood-branch" {
                Command::new(std::env::current_exe()?)
                    .args(["delayed-long", &args[1]])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
            }
            let bytes = vec![123; 32768];
            for _ in 0..96 {
                io::stdout().write_all(&bytes)?;
            }
            for _ in 0..32 {
                io::stderr().write_all(&bytes)?;
            }
            if args[0] == "flood-branch" {
                std::thread::sleep(Duration::from_secs(30));
            }
        }
        Some("branch") | Some("idle-branch") => {
            let marker = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Missing owned marker."))?;
            let mut child = Command::new(std::env::current_exe()?);
            child.args(["delayed", marker]);
            if args.get(2).is_some_and(|s| s == "null") {
                child
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
            }
            child.spawn()?;
            io::stdout().write_all(b"ROOT_READY\n")?;
            io::stdout().flush()?;
            if args[0] == "idle-branch" {
                std::thread::sleep(Duration::from_secs(30));
            }
        }
        Some("delayed") | Some("delayed-long") => {
            std::thread::sleep(if args[0] == "delayed-long" {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(1500)
            });
            std::fs::write(&args[1], b"uncontained descendant")?;
        }
        Some("idle") => std::thread::sleep(Duration::from_secs(30)),
        Some("eof") | Some("deferred-eof") => {
            if args[0] == "deferred-eof" {
                std::thread::sleep(Duration::from_millis(1500));
            }
            io::copy(&mut io::stdin(), &mut io::sink())?;
            io::stdout().write_all(b"EOF")?;
        }
        _ => anyhow::bail!("Unknown owned fixture mode."),
    }
    Ok(())
}
