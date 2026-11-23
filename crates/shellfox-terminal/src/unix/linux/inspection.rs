//! Read-only process evidence for a Linux PTY. The cgroup is the membership
//! boundary; /proc fields are used for matching agents, never for signalling.
use super::{super::tree::Record, procfs};
use crate::inspection::{
    InspectionFailure as Error, InspectionPlatform, RawOwnedEvidence, RawProcessEvidence,
    RawProcessIdentity, MAX_INSPECTION_BYTES, MAX_INSPECTION_FIELD_BYTES, MAX_INSPECTION_PROCESSES,
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::File,
    io::Read,
    os::{fd::AsRawFd, unix::ffi::OsStringExt},
    sync::OnceLock,
};

pub(crate) struct OwnedInspection {
    group: File,
    supervisor: File,
    pid: u32,
    uid: u32,
    shell: OnceLock<RawProcessIdentity>,
}

impl OwnedInspection {
    pub fn new(group: File, supervisor: File, pid: u32) -> Self {
        // SAFETY: getuid is a read-only kernel query.
        Self {
            group,
            supervisor,
            pid,
            uid: unsafe { libc::getuid() },
            shell: OnceLock::new(),
        }
    }

    fn supervisor_alive(&self) -> bool {
        let mut fd = libc::pollfd {
            fd: self.supervisor.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `fd` is live for one nonblocking read-only poll of the original pidfd.
        unsafe { libc::poll(&mut fd, 1, 0) == 0 }
    }

    fn members(&self) -> Result<BTreeSet<u32>, Error> {
        const MAX: u64 = (MAX_INSPECTION_PROCESSES * 12) as u64;
        let mut text = String::new();
        super::super::cgroup::open_at(&self.group, c"cgroup.procs", libc::O_RDONLY)
            .map_err(|_| Error::Unavailable)?
            .take(MAX + 1)
            .read_to_string(&mut text)
            .map_err(|_| Error::Unavailable)?;
        if text.len() as u64 > MAX || text.lines().count() > MAX_INSPECTION_PROCESSES {
            return Err(Error::Bounds);
        }
        text.lines()
            .map(|row| row.parse().map_err(|_| Error::Malformed))
            .collect()
    }

    pub fn capture(&self) -> Result<RawOwnedEvidence, Error> {
        if !self.supervisor_alive() {
            return Err(Error::Unavailable);
        }
        let members = self.members()?;
        if !members.contains(&self.pid) {
            return Err(Error::OwnershipMismatch);
        }
        let mut complete = true;
        let mut total = 0;
        let mut root = self.shell.get().copied();
        let mut candidates = Vec::new();
        let mut processes = Vec::new();
        for pid in members.iter().copied().filter(|pid| *pid != self.pid) {
            match process(pid, self.uid, &mut total) {
                Ok((record, mut row)) => {
                    if root.is_none() && record.parent == self.pid && record.session == self.pid {
                        candidates.push(row.identity);
                    }
                    row.is_root = Some(row.identity) == root;
                    processes.push(row);
                }
                Err(Error::Bounds) => return Err(Error::Bounds),
                Err(_) => complete = false,
            }
        }
        let after = self.members()?;
        if !self.supervisor_alive() || !after.contains(&self.pid) {
            return Err(Error::Unavailable);
        }
        if root.is_none() {
            let [shell] = candidates.as_slice() else {
                return Err(Error::Pending);
            };
            if !after.contains(&shell.pid) {
                return Err(Error::Pending);
            }
            let _ = self.shell.set(*shell);
            root = self.shell.get().copied();
        }
        let root = root.ok_or(Error::Pending)?;
        complete &= members == after;
        processes.retain(|row| after.contains(&row.identity.pid));
        let mut root_alive = false;
        for row in &mut processes {
            row.is_root = row.identity == root;
            root_alive |= row.is_root;
        }
        if !root_alive {
            // A transient unreadable original root is not an exit observation.
            if after.contains(&root.pid) {
                return Err(Error::Unavailable);
            }
            processes.push(RawProcessEvidence {
                identity: root,
                is_root: true,
                accessible: false,
                executable: None,
                process_name: None,
                argv: None,
                parent_pid: None,
                memory_bytes: None,
                started_ms: None,
            });
        }
        Ok(RawOwnedEvidence {
            platform: InspectionPlatform::Unix,
            root,
            root_alive,
            complete,
            processes,
        })
    }
}

fn process(pid: u32, uid: u32, total: &mut usize) -> Result<(Record, RawProcessEvidence), Error> {
    let read = || procfs::record(pid, uid).map_err(|_| Error::Unavailable);
    let before = read()?;
    if before.dead() {
        return Err(Error::Unavailable);
    }
    let path = |name: &str| format!("/proc/{pid}/{name}");
    let executable = std::fs::read_link(path("exe"))
        .map_err(|_| Error::Unavailable)?
        .into_os_string();
    let name = procfs::bounded(path("comm"), MAX_INSPECTION_FIELD_BYTES as u64)
        .map_err(|_| Error::Unavailable)?;
    let cmdline = procfs::bounded(path("cmdline"), MAX_INSPECTION_FIELD_BYTES as u64)
        .map_err(|_| Error::Unavailable)?;
    let after = read()?;
    if after.identity != before.identity
        || after.parent != before.parent
        || after.session != before.session
        || after.dead()
    {
        return Err(Error::OwnershipMismatch);
    }
    *total += executable.len() + name.len() + cmdline.len();
    if *total > MAX_INSPECTION_BYTES {
        return Err(Error::Bounds);
    }
    let suffix = b" (deleted)";
    let image = executable.as_encoded_bytes();
    let executable = OsString::from_vec(image.strip_suffix(suffix).unwrap_or(image).to_vec());
    let argv = cmdline
        .strip_suffix(b"\0")
        .unwrap_or(&cmdline)
        .split(|b| *b == 0)
        .map(|arg| OsString::from_vec(arg.to_vec()))
        .collect::<Vec<_>>();
    if argv.len() > 4096 {
        return Err(Error::Bounds);
    }
    let row = RawProcessEvidence {
        parent_pid: Some(after.parent),
        memory_bytes: resident(pid),
        started_ms: started_ms(before.identity.ticks),
        identity: RawProcessIdentity {
            pid,
            birth: before.identity.ticks,
        },
        is_root: false,
        accessible: !cmdline.is_empty(),
        executable: Some(executable),
        process_name: Some(OsString::from_vec(
            name.strip_suffix(b"\n").unwrap_or(&name).to_vec(),
        )),
        argv: (!cmdline.is_empty()).then_some(argv),
    };
    Ok((after, row))
}

/// Resident set from /proc/<pid>/statm, in bytes. The caller has checked the owner.
fn resident(pid: u32) -> Option<u64> {
    let text = procfs::bounded(format!("/proc/{pid}/statm"), 256).ok()?;
    let pages: u64 = std::str::from_utf8(&text)
        .ok()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    // SAFETY: sysconf is a read-only query.
    let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
    pages.checked_mul(page)
}
/// Start ticks since boot as ms since the Unix epoch, using btime from /proc/stat.
fn started_ms(ticks: u64) -> Option<u64> {
    static BOOT: OnceLock<Option<(u64, u64)>> = OnceLock::new();
    let (boot, hertz) = (*BOOT.get_or_init(|| {
        let text = procfs::bounded("/proc/stat", 1 << 20).ok()?;
        let boot = std::str::from_utf8(&text)
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("btime "))?
            .trim()
            .parse()
            .ok()?;
        // SAFETY: sysconf is a read-only query.
        let hertz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) })
            .ok()
            .filter(|h| *h > 0)?;
        Some((boot, hertz))
    }))?;
    boot.checked_mul(1000)?
        .checked_add(ticks.checked_mul(1000)? / hertz)
}

#[cfg(test)]
mod tests {
    #[test]
    fn procfs_reports_read_only_identity_and_argv_for_this_process() {
        let mut total = 0;
        // SAFETY: getuid is read-only.
        let uid = unsafe { libc::getuid() };
        let (_, row) = super::process(std::process::id(), uid, &mut total).unwrap();
        assert_ne!(row.identity.birth, 0);
        assert!(row.accessible && row.executable.is_some() && row.argv.is_some());
        assert!(row.parent_pid.is_some() && row.memory_bytes.is_some_and(|m| m > 0));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(row
            .started_ms
            .is_some_and(|s| s <= now + 1000 && s + 3_600_000 > now));
        assert!(total > 0);
    }
}
