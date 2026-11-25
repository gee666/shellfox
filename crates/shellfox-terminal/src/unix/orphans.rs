//! Terminal cgroups left behind by a host that died together with their
//! controllers (`killall shellfox-cli`, for example). A controller always ends
//! its own cgroup, and a live host ends the cgroup of a controller that died,
//! so a populated terminal cgroup without a controller has no owner left.
//!
//! Each cgroup records its host's data directory in a user xattr. A starting
//! host ends only its own data directory's ownerless cgroups, so it never
//! touches another host's terminals, and the cgroups live in this account's
//! delegation, never another user's.
use super::cgroup::{delegation, open_at, OWNER_XATTR};
use anyhow::{Context, Result};
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static OWNER: OnceLock<Vec<u8>> = OnceLock::new();

/// The xattr value new terminal cgroups get, once this process hosts terminals.
pub(super) fn owner() -> Option<&'static [u8]> {
    OWNER.get().map(Vec::as_slice)
}

/// Records `data_dir` as the owner of this process's terminal cgroups and
/// ends ownerless ones a previous host of it left behind. Call once, at host
/// start, before any terminal starts.
pub fn adopt(data_dir: &Path) {
    let tag = data_dir.as_os_str().as_bytes().to_vec();
    if OWNER.set(tag.clone()).is_err() {
        return;
    }
    match sweep(&tag) {
        Ok(0) => {}
        Ok(ended) => eprintln!("Ended {ended} terminal group(s) a previous host left running"),
        Err(error) => eprintln!("Terminal group sweep skipped: {error:#}"),
    }
}

fn sweep(tag: &[u8]) -> Result<usize> {
    let Ok(parent) = delegation() else {
        return Ok(0); // No delegation: session containment, nothing to sweep.
    };
    let mut ended = 0;
    let entries = std::fs::read_dir(format!("/proc/self/fd/{}", parent.as_raw_fd()))?;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !is_terminal_group(name.as_bytes()) {
            continue;
        }
        let name = CString::new(name.as_bytes())?;
        let Ok(group) = open_at(&parent, &name, libc::O_RDONLY | libc::O_DIRECTORY) else {
            continue;
        };
        if owner_of(&group).as_deref() != Some(tag) {
            continue;
        }
        match end(&parent, &group, &name) {
            Ok(true) => ended += 1,
            Ok(false) => {}
            Err(error) => eprintln!("Terminal group {name:?} not ended: {error:#}"),
        }
    }
    Ok(ended)
}

/// `shellfox-<64 hex>.scope`, the name `Cgroup::create` gives.
pub(super) fn is_terminal_group(name: &[u8]) -> bool {
    name.strip_prefix(b"shellfox-")
        .and_then(|rest| rest.strip_suffix(b".scope"))
        .is_some_and(|token| token.len() == 64 && token.iter().all(u8::is_ascii_hexdigit))
}

fn owner_of(group: &std::fs::File) -> Option<Vec<u8>> {
    let mut value = vec![0u8; 4096];
    // SAFETY: the buffer is valid for its length; the name is NUL-terminated.
    let length = unsafe {
        libc::fgetxattr(
            group.as_raw_fd(),
            OWNER_XATTR.as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
        )
    };
    value.truncate(usize::try_from(length).ok()?);
    Some(value)
}

/// Kills an ownerless group and removes it. A group with a live controller is
/// left alone. True if it held processes.
fn end(parent: &std::fs::File, group: &std::fs::File, name: &CString) -> Result<bool> {
    let pids = members(group)?;
    if pids.iter().any(|pid| is_controller(*pid)) {
        return Ok(false);
    }
    let populated = !pids.is_empty();
    if populated {
        open_at(group, c"cgroup.kill", libc::O_WRONLY)?
            .write_all(b"1")
            .context("kill terminal group")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !members(group)?.is_empty() {
            anyhow::ensure!(Instant::now() < deadline, "terminal group did not empty");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // SAFETY: a name inside the held parent directory; fails if not empty.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        anyhow::bail!("remove: {}", std::io::Error::last_os_error());
    }
    Ok(populated)
}

fn members(group: &std::fs::File) -> Result<Vec<u32>> {
    let mut text = String::new();
    open_at(group, c"cgroup.procs", libc::O_RDONLY)?.read_to_string(&mut text)?;
    Ok(text.lines().filter_map(|line| line.parse().ok()).collect())
}

/// A PTY or raw terminal controller, recognised by its flag.
fn is_controller(pid: u32) -> bool {
    let Ok(command) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let flag = command.split(|byte| *byte == 0).nth(1).unwrap_or_default();
    [super::startup::FLAG, super::raw::FLAG]
        .iter()
        .any(|known| flag == known.as_bytes())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_terminal_group_names_are_swept() {
        let token = "a".repeat(64);
        assert!(super::is_terminal_group(
            format!("shellfox-{token}.scope").as_bytes()
        ));
        for name in [
            "shellfox-host-6f1c7dac-c7ca-4f92-b1ec-3ce0b4877587.scope".to_owned(),
            format!("shellfox-{}.scope", "g".repeat(64)),
            format!("shellfox-{token}.slice"),
            "app.slice".to_owned(),
        ] {
            assert!(!super::is_terminal_group(name.as_bytes()), "{name}");
        }
    }
}
