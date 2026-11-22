//! A kernel-owned descendant boundary, independent of ancestry and sessions.
//! Requires an existing systemd user delegation. Never changes its controllers.
use anyhow::{ensure, Context, Result};
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

pub(super) struct Cgroup {
    parent: File,
    directory: File,
    name: CString,
    procs: File,
    kill: File,
    events: File,
}
pub(super) fn open_at(directory: &File, name: &CStr, flags: i32) -> Result<File> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    ensure!(
        fd >= 0,
        "open cgroup file: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { File::from_raw_fd(fd) })
}
impl Cgroup {
    pub fn create() -> Result<Self> {
        let uid = unsafe { libc::getuid() };
        ensure!(
            uid != 0 && uid == unsafe { libc::geteuid() },
            "PTY requires a non-elevated user delegation"
        );
        let path = format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service");
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)
            .context("Linux PTY requires a writable cgroup-v2 systemd user delegation")?;
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        ensure!(
            unsafe { libc::fstatfs(parent.as_raw_fd(), stat.as_mut_ptr()) } == 0,
            "inspect cgroup filesystem"
        );
        ensure!(
            unsafe { stat.assume_init() }.f_type == libc::CGROUP2_SUPER_MAGIC
                && parent.metadata()?.uid() == uid,
            "not an owned cgroup-v2 delegation"
        );
        let name = CString::new(format!(
            "shellfox-{}.scope",
            super::private::random_token()?
        ))?;
        ensure!(
            unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
            "create private PTY cgroup: {}",
            std::io::Error::last_os_error()
        );
        let opened = (|| {
            let directory = open_at(&parent, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
            let procs = open_at(&directory, c"cgroup.procs", libc::O_WRONLY)?;
            let kill = open_at(&directory, c"cgroup.kill", libc::O_WRONLY)?;
            let events = open_at(&directory, c"cgroup.events", libc::O_RDONLY)?;
            // An OOM kill takes only the chosen process, never the whole terminal
            // tree or a sibling. This cgroup is not a systemd unit, so no OOMPolicy
            // applies, and user@.service (Delegate=yes) defaults to OOMPolicy=continue.
            // The file is missing when the memory controller is not delegated.
            if let Ok(mut group) = open_at(&directory, c"memory.oom.group", libc::O_WRONLY) {
                group.write_all(b"0").context("keep per-process OOM kills")?;
            }
            Ok(Self {
                parent: parent.try_clone()?,
                directory,
                name: name.clone(),
                procs,
                kill,
                events,
            })
        })();
        if opened.is_err() {
            // No process has been launched or attached yet.
            unsafe {
                libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR);
            }
        }
        opened
    }
    pub fn inspection_directory(&self) -> Result<File> {
        Ok(self.directory.try_clone()?)
    }
    pub fn attach(&mut self, pid: u32) -> Result<()> {
        self.procs
            .write_all(pid.to_string().as_bytes())
            .context("attach waiting original PTY child")
    }
    pub fn terminate(&mut self) -> Result<()> {
        self.kill.write_all(b"1").context("kill owned PTY cgroup")
    }
    pub fn empty(&mut self) -> Result<bool> {
        self.events.seek(SeekFrom::Start(0))?;
        let mut value = String::new();
        Read::by_ref(&mut self.events)
            .take(4096)
            .read_to_string(&mut value)?;
        ensure!(
            value
                .lines()
                .any(|line| matches!(line, "populated 0" | "populated 1")),
            "cgroup populated proof missing"
        );
        Ok(value.lines().any(|line| line == "populated 0"))
    }
    pub fn remove(&mut self) -> Result<()> {
        ensure!(self.empty()?, "PTY cgroup is still populated");
        // Verify the pinned directory before deleting its unique directory entry.
        let current = open_at(&self.parent, &self.name, libc::O_RDONLY | libc::O_DIRECTORY)?;
        ensure!(
            current.metadata()?.ino() == self.directory.metadata()?.ino(),
            "PTY cgroup identity changed"
        );
        ensure!(
            unsafe {
                libc::unlinkat(
                    self.parent.as_raw_fd(),
                    self.name.as_ptr(),
                    libc::AT_REMOVEDIR,
                )
            } == 0,
            "remove empty PTY cgroup: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
}
impl Drop for Cgroup {
    fn drop(&mut self) {
        // Best effort only for pre-launch errors. Live owners are retained by State.
        if self.empty().unwrap_or(false) {
            let _ = self.remove();
        }
    }
}
