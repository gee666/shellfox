use crate::unix::private::PrivateDirectory;
use crate::unix::{ControlCommand, ControlIdentity, SupervisorStatus};
use anyhow::{ensure, Context, Result};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub(super) struct Control {
    pub directory: PrivateDirectory,
    pub token: String,
    pub supervisor_pid: Option<u32>,
    pub identity: Option<ControlIdentity>,
}
impl Control {
    pub fn request(&self, command: ControlCommand, timeout: Duration) -> Result<SupervisorStatus> {
        self.directory.unchanged()?;
        let pid = self
            .supervisor_pid
            .context("supervisor child identity not bound")?;
        // Nonblocking connect also bounds a full listener backlog. A blocking
        // UnixStream::connect has no timeout and could otherwise hang cleanup.
        let stream = connect(&self.directory.socket(), timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        // SAFETY: valid connected socket and initialized output credentials.
        ensure!(
            unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0
                && uid == unsafe { libc::getuid() },
            "supervisor peer credentials changed"
        );
        let mut stream = stream;
        stream.write_all(command.frame(&self.token)?.as_bytes())?;
        let deadline = Instant::now() + timeout;
        let mut bytes = Vec::new();
        loop {
            ensure!(
                Instant::now() < deadline,
                "supervisor response deadline exceeded; owner retained"
            );
            stream.set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1)),
            ))?;
            let mut chunk = [0u8; 512];
            let count = stream.read(&mut chunk)?;
            if count == 0 {
                return SupervisorStatus::parse(&bytes, pid, self.identity.as_ref());
            }
            bytes.extend_from_slice(&chunk[..count]);
            ensure!(
                bytes.len() <= 2048,
                "supervisor control frame exceeded its bound"
            );
            // The active helper closes each connection after its single reply.
            // Read through EOF so trailing frames in later packets are refused.
        }
    }
    pub fn ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(
                !remaining.is_zero(),
                "supervisor shell startup could not be authenticated"
            );
            let result = self
                .request(ControlCommand::Status, remaining)
                .and_then(|status| status.ready_identity());
            match result {
                Ok(identity) => {
                    self.identity = Some(identity);
                    return Ok(());
                }
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
    pub fn close(&mut self) -> Result<()> {
        self.request(ControlCommand::Close, Duration::from_secs(15))?
            .confirm_closed()?;
        // The reply can arrive just before the helper unlinks its endpoint.
        // Retain the directory until that final cleanup has actually happened.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            self.directory.unchanged()?;
            match std::fs::symlink_metadata(self.directory.socket()) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
                Ok(_) => (),
            }
            ensure!(
                Instant::now() < deadline,
                "supervisor endpoint cleanup not confirmed; control retained"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn connect(path: &std::path::Path, timeout: Duration) -> Result<UnixStream> {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    ensure!(
        bytes.len() < 104 && !bytes.contains(&0),
        "invalid supervisor socket path"
    );
    // SAFETY: zero initializes sockaddr_un padding and the NUL path terminator.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    address.sun_len = std::mem::size_of::<libc::sockaddr_un>() as u8;
    for (dest, src) in address.sun_path.iter_mut().zip(bytes) {
        *dest = *src as libc::c_char;
    }
    // SAFETY: valid socket type; returned descriptor is immediately owned.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let stream = UnixStream::from(fd);
    let no_sigpipe: libc::c_int = 1;
    // SAFETY: initialized option storage, valid socket and option length.
    ensure!(
        unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                &no_sigpipe as *const _ as *const libc::c_void,
                std::mem::size_of_val(&no_sigpipe) as libc::socklen_t,
            )
        } == 0,
        "cannot disable control socket SIGPIPE"
    );
    stream.set_nonblocking(true)?;
    // SAFETY: address points to one initialized sockaddr_un of the supplied size.
    let result = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            &address as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        ensure!(
            matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EAGAIN)),
            "control connect failed: {error}"
        );
        let mut poll = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: poll points to one initialized event.
        ensure!(
            unsafe { libc::poll(&mut poll, 1, millis) } > 0,
            "control connect timed out"
        );
        if let Some(error) = stream.take_error()? {
            return Err(error.into());
        }
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}
