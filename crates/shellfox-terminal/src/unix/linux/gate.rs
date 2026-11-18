//! Waiting exec gate. The interactive command cannot run until its original
//! PTY process has an authenticated pidfd owner in the host.
use super::LinuxOwner;
use crate::unix::private::PrivateDirectory;
use anyhow::{ensure, Context, Result};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const WAIT_FLAG: &str = "--shellfox-unix-wait-v1";

pub struct StartupGate {
    listener: UnixListener,
    directory: PrivateDirectory,
    socket_device: u64,
    socket_inode: u64,
    control: Option<UnixStream>,
}
impl StartupGate {
    /// Call before PTY allocation. `runtime` must be the isolated runtime root.
    pub fn prepare(runtime: &Path) -> Result<Self> {
        let directory = PrivateDirectory::create(runtime)?;
        let listener = UnixListener::bind(directory.socket())?;
        listener.set_nonblocking(true)?;
        let info = std::fs::symlink_metadata(directory.socket())?;
        Ok(Self {
            listener,
            directory,
            socket_device: info.dev(),
            socket_inode: info.ino(),
            control: None,
        })
    }
    pub fn arguments(&self, command: &Path, args: &[OsString]) -> Result<Vec<OsString>> {
        ensure!(
            command.is_absolute(),
            "waiting command path must be absolute"
        );
        let mut argv = vec![
            WAIT_FLAG.into(),
            self.directory.socket().into_os_string(),
            std::process::id().to_string().into(),
            command.as_os_str().to_owned(),
        ];
        argv.extend_from_slice(args);
        Ok(argv)
    }
    pub fn release(&mut self, child_pid: u32, owner: &mut LinuxOwner) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            ensure!(
                Instant::now() < deadline,
                "waiting supervisor connect deadline exceeded"
            );
            self.directory.unchanged()?;
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let peer = peer(&stream)?;
                    if peer.pid != child_pid as libc::pid_t || peer.uid != unsafe { libc::getuid() }
                    {
                        continue;
                    }
                    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                    let mut ready = [0u8; 6];
                    stream.read_exact(&mut ready)?;
                    ensure!(&ready == b"READY\n", "invalid waiting supervisor frame");
                    owner.capture_root(child_pid)?;
                    // Ownership capture, marker and ancestry rechecks all precede
                    // GO. A failed send keeps the same pidfd for close/retry.
                    stream.write_all(b"GO\n")?;
                    let mut frame = Vec::new();
                    loop {
                        let mut byte = [0u8; 1];
                        stream.read_exact(&mut byte)?;
                        frame.push(byte[0]);
                        ensure!(frame.len() <= 128, "shell startup frame exceeded its bound");
                        if byte[0] == b'\n' {
                            break;
                        }
                    }
                    let frame = std::str::from_utf8(&frame)?;
                    let fields: Vec<_> = frame.trim_end_matches('\n').split('\t').collect();
                    ensure!(
                        fields.len() == 3 && fields[0] == "START",
                        "owned command exec failed"
                    );
                    let shell_pid = fields[1].parse()?;
                    let shell_ticks = fields[2].parse()?;
                    self.control = Some(stream);
                    owner.capture_shell(shell_pid, shell_ticks)?;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => return Err(error.into()),
            }
            ensure!(
                Instant::now() < deadline,
                "waiting supervisor did not connect; command was not released"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for StartupGate {
    fn drop(&mut self) {
        if self.directory.unchanged().is_ok() {
            if let Ok(info) = std::fs::symlink_metadata(self.directory.socket()) {
                if info.dev() == self.socket_device && info.ino() == self.socket_inode {
                    let _ = std::fs::remove_file(self.directory.socket());
                }
            }
        }
    }
}

/// The trusted supervisor binary calls this before GUI initialization/threads.
/// The host invokes only this waiting executable in the PTY, never the shell
/// directly. On EOF, timeout or wrong peer credentials the command is not exec'd.
pub fn waiting_entry() -> Result<Option<u32>> {
    let mut argv = std::env::args_os().skip(1);
    if argv.next().as_deref() != Some(std::ffi::OsStr::new(WAIT_FLAG)) {
        return Ok(None);
    }
    let endpoint = PathBuf::from(argv.next().context("waiting socket path missing")?);
    let parent: u32 = argv
        .next()
        .context("waiting parent missing")?
        .to_str()
        .context("invalid parent")?
        .parse()?;
    let command = PathBuf::from(argv.next().context("waiting command missing")?);
    ensure!(
        command.is_absolute() && endpoint.is_absolute(),
        "waiting paths must be absolute"
    );
    ensure!(
        unsafe { libc::getuid() } == unsafe { libc::geteuid() }
            && unsafe { libc::getsid(0) } == std::process::id() as libc::pid_t
            && unsafe { libc::getppid() } == parent as libc::pid_t,
        "waiting supervisor is not the original PTY child"
    );
    let mut stream = UnixStream::connect(endpoint)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let credentials = peer(&stream)?;
    ensure!(
        credentials.pid == parent as libc::pid_t && credentials.uid == unsafe { libc::getuid() },
        "waiting owner peer changed"
    );
    stream.write_all(b"READY\n")?;
    let mut go = [0u8; 3];
    stream.read_exact(&mut go)?;
    ensure!(&go == b"GO\n", "waiting supervisor was not released");
    // The persistent subreaper does not exec the shell in place. It remains
    // the exact live session anchor until every descendant has exited.
    super::supervisor::run(&mut stream, &command, argv).map(Some)
}
fn peer(stream: &UnixStream) -> Result<libc::ucred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the socket is held and outputs point to initialized storage of the
    // required size. Linux returns credentials captured at connection time.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credentials as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    ensure!(
        result == 0 && length as usize == std::mem::size_of::<libc::ucred>(),
        "waiting peer credentials unavailable"
    );
    Ok(credentials)
}
