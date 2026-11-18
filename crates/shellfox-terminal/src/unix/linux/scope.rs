//! Optional systemd user-scope isolation, independent of process ownership.
use anyhow::{ensure, Result};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const TERMINAL_OOM_SCORE_ADJ: i32 = 300;

/// Existing behavior: test the first executable systemd-run found in absolute
/// PATH entries and standard directories; use a direct owned shell on refusal.
/// The caller can instead supply its own explicitly probed runner.
pub fn discover(search_path: &std::ffi::OsStr, true_program: &Path) -> Result<Option<PathBuf>> {
    let directories = std::env::split_paths(search_path)
        .chain([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    for directory in directories.filter(|p| p.is_absolute()) {
        let candidate = directory.join("systemd-run");
        let Ok(candidate) = crate::unix::private::helper_path(&candidate) else {
            continue;
        };
        return probe(&candidate, true_program, &fresh_nonce()?);
    }
    Ok(None)
}
pub fn fresh_nonce() -> Result<String> {
    let value = crate::unix::private::random_token()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &value[..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..32]
    ))
}

pub fn scope_arguments(
    unit_nonce: &str,
    program: &Path,
    args: &[OsString],
) -> Result<Vec<OsString>> {
    ensure!(
        unit_nonce.len() == 36
            && unit_nonce.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            }),
        "scope unit must use a fresh UUID"
    );
    ensure!(program.is_absolute(), "scope command must be absolute");
    let mut result: Vec<OsString> = ["--user", "--scope", "--quiet", "--collect"]
        .into_iter()
        .map(Into::into)
        .collect();
    result.extend([
        format!("--unit=shellfox-terminal-{unit_nonce}").into(),
        "--description=Shellfox terminal".into(),
        "--property=OOMPolicy=continue".into(),
        "--".into(),
        program.as_os_str().to_owned(),
    ]);
    result.extend_from_slice(args);
    Ok(result)
}

/// Explicit paths only. Caller may discover candidates from absolute PATH
/// entries, but must not allow cwd lookup or use a probe result as kill authority.
/// The same options used for launch are tested against a disposable `true`.
pub fn probe(runner: &Path, true_program: &Path, fresh_nonce: &str) -> Result<Option<PathBuf>> {
    ensure!(
        runner.is_absolute() && true_program.is_absolute(),
        "scope probe paths must be absolute"
    );
    let args = scope_arguments(fresh_nonce, true_program, &[])?;
    let mut child = match Command::new(runner)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Ok(None),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success().then(|| runner.to_owned()));
        }
        if Instant::now() >= deadline {
            // This is our unreaped direct probe child, never a cached target PID.
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Best effort, raise only. Bind /proc accesses to one open process directory
/// and verify its birth before opening the writable file. An exited process's
/// directory descriptor cannot redirect this write to a recycled numeric PID.
pub(super) fn raise_owned_oom_score(pid: u32, ticks: u64, value: i32) {
    let _ = raise_score(pid, ticks, value);
}
fn raise_score(pid: u32, ticks: u64, value: i32) -> Result<()> {
    ensure!((0..=1000).contains(&value), "invalid OOM score");
    let directory = std::fs::File::open(format!("/proc/{pid}"))?;
    let mut stat = String::new();
    open_at(&directory, c"stat", libc::O_RDONLY)?
        .take(16385)
        .read_to_string(&mut stat)?;
    ensure!(
        stat.len() <= 16384 && super::procfs::parse_stat(&stat, pid)?.identity.ticks == ticks,
        "OOM process identity changed"
    );
    let mut score = open_at(&directory, c"oom_score_adj", libc::O_RDWR)?;
    let mut previous = String::new();
    (&mut score).take(64).read_to_string(&mut previous)?;
    if previous.trim().parse::<i32>()? < value {
        use std::io::{Seek, SeekFrom};
        score.seek(SeekFrom::Start(0))?;
        score.write_all(value.to_string().as_bytes())?;
    }
    Ok(())
}
fn open_at(directory: &std::fs::File, leaf: &std::ffi::CStr, flags: i32) -> Result<std::fs::File> {
    // SAFETY: directory is held, leaf is NUL terminated, no creation is requested.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            leaf.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    Ok(std::fs::File::from(owned))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_does_not_reinterpret_command_arguments() {
        let args = scope_arguments(
            "12345678-1234-1234-1234-123456789abc",
            Path::new("/bin/sh"),
            &["--user".into()],
        )
        .unwrap();
        assert!(args.contains(&"--property=OOMPolicy=continue".into()));
        assert_eq!(
            &args[args.len() - 3..],
            &[OsString::from("--"), "/bin/sh".into(), "--user".into()]
        );
        assert!(scope_arguments("../bad", Path::new("/bin/sh"), &[]).is_err());
    }
}
