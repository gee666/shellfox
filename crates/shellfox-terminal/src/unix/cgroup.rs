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
/// Names the data directory of the host that created a terminal cgroup (orphans.rs).
pub(super) const OWNER_XATTR: &CStr = c"user.shellfox.owner";
/// This account's systemd user delegation.
pub(super) fn delegation() -> Result<File> {
    let uid = unsafe { libc::getuid() };
    ensure!(
        uid == unsafe { libc::geteuid() },
        "PTY cgroup requires matching real and effective user IDs"
    );
    let path = format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service");
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .context("no systemd user delegation")?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    ensure!(
        unsafe { libc::fstatfs(parent.as_raw_fd(), stat.as_mut_ptr()) } == 0,
        "inspect cgroup filesystem"
    );
    // f_type and the magic have different integer types on gnu and musl.
    let filesystem = i128::from(unsafe { stat.assume_init() }.f_type);
    ensure!(
        filesystem == i128::from(libc::CGROUP2_SUPER_MAGIC) && parent.metadata()?.uid() == uid,
        "not an owned cgroup-v2 delegation"
    );
    Ok(parent)
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
        let parent = delegation()?;
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
            if let Some(owner) = super::orphans::owner() {
                // SAFETY: a held directory, a NUL-terminated name and a valid buffer.
                let set = unsafe {
                    libc::fsetxattr(
                        directory.as_raw_fd(),
                        OWNER_XATTR.as_ptr(),
                        owner.as_ptr().cast(),
                        owner.len(),
                        0,
                    )
                };
                // Without it the group still works; only the orphan sweep skips it.
                static REPORTED: std::sync::Once = std::sync::Once::new();
                if set != 0 {
                    let error = std::io::Error::last_os_error();
                    REPORTED.call_once(|| eprintln!("Terminal group owner not recorded: {error}"));
                }
            }
            let procs = open_at(&directory, c"cgroup.procs", libc::O_WRONLY)?;
            let kill = open_at(&directory, c"cgroup.kill", libc::O_WRONLY)?;
            let events = open_at(&directory, c"cgroup.events", libc::O_RDONLY)?;
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
    /// Calls `wake` on every cgroup.events change until the group is empty.
    pub fn watch(&self, wake: crate::Wake) -> Result<()> {
        // Its own open file: kernfs tracks seen events per open file, so reads
        // through `self.events` must not consume the watcher's notification.
        let mut events = open_at(&self.directory, c"cgroup.events", libc::O_RDONLY)?;
        std::thread::Builder::new()
            .name("shellfox-cgroup-watch".into())
            .spawn(move || {
                while matches!(populated(&mut events), Ok(true)) {
                    let mut poll = libc::pollfd {
                        fd: events.as_raw_fd(),
                        events: libc::POLLPRI,
                        revents: 0,
                    };
                    // SAFETY: one valid pollfd for the open events file.
                    if unsafe { libc::poll(&mut poll, 1, -1) } < 0
                        && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        return;
                    }
                    wake();
                }
                wake();
            })
            .context("start cgroup watch")?;
        Ok(())
    }
    pub fn terminate(&mut self) -> Result<()> {
        self.kill.write_all(b"1").context("kill owned PTY cgroup")
    }
    pub fn empty(&mut self) -> Result<bool> {
        Ok(!populated(&mut self.events)?)
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
fn populated(events: &mut File) -> Result<bool> {
    events.seek(SeekFrom::Start(0))?;
    let mut value = String::new();
    Read::by_ref(events).take(4096).read_to_string(&mut value)?;
    ensure!(
        value
            .lines()
            .any(|line| matches!(line, "populated 0" | "populated 1")),
        "cgroup populated proof missing"
    );
    Ok(value.lines().any(|line| line == "populated 1"))
}
impl Drop for Cgroup {
    fn drop(&mut self) {
        // Best effort only for pre-launch errors. Live owners are retained by State.
        if self.empty().unwrap_or(false) {
            let _ = self.remove();
        }
    }
}
