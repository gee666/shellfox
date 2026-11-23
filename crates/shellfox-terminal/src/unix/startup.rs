//! Linux abstract socket capability plus exact SO_PEERCRED, never a PID lookup.
use anyhow::{ensure, Result};
use std::os::fd::AsRawFd;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::time::{Duration, Instant};

pub(super) const FLAG: &str = "--shellfox-linux-pty-v1";
pub(super) struct Gate {
    listener: UnixListener,
    name: String,
}
pub(super) fn peer(stream: &UnixStream) -> Result<libc::ucred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        } == 0
            && length as usize == std::mem::size_of::<libc::ucred>(),
        "PTY peer credentials unavailable"
    );
    Ok(credentials)
}
fn privilege_argument(mode: crate::UnixPrivilegeMode) -> &'static str {
    match mode {
        crate::UnixPrivilegeMode::Local => "local",
        crate::UnixPrivilegeMode::NonElevating => "non-elevating",
    }
}
impl Gate {
    pub fn new() -> Result<Self> {
        let name = format!("shellfox-{}", super::private::random_token()?);
        let listener = UnixListener::bind_addr(&SocketAddr::from_abstract_name(&name)?)?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener, name })
    }
    pub fn arguments(
        &self,
        command: &crate::PtyCommand,
        mode: crate::UnixPrivilegeMode,
    ) -> Vec<std::ffi::OsString> {
        let mut args = vec![
            FLAG.into(),
            self.name.clone().into(),
            std::process::id().to_string().into(),
            privilege_argument(mode).into(),
            command.program.clone(),
        ];
        args.extend_from_slice(&command.args);
        args
    }
    pub fn release(&self, pid: u32, before_go: impl FnOnce() -> Result<()>) -> Result<UnixStream> {
        self.release_raw(pid, &[], before_go)
    }
    pub fn raw_arguments(&self, mode: crate::UnixPrivilegeMode) -> Vec<std::ffi::OsString> {
        vec![
            super::raw::FLAG.into(),
            self.name.clone().into(),
            std::process::id().to_string().into(),
            privilege_argument(mode).into(),
        ]
    }
    pub fn release_raw(
        &self,
        pid: u32,
        payload: &[u8],
        before_go: impl FnOnce() -> Result<()>,
    ) -> Result<UnixStream> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let credentials = peer(&stream)?;
                    if credentials.pid != pid as libc::pid_t
                        || credentials.uid != unsafe { libc::getuid() }
                    {
                        continue;
                    }
                    return super::handshake::release(stream, payload, before_go);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => return Err(error.into()),
            }
            ensure!(
                Instant::now() < deadline,
                "owned PTY startup timed out before GO"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
