//! Use short, descriptor-relative socket names without changing process cwd.
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, net as unix_net},
    },
    path::Path,
};
use tokio::net::{UnixListener, UnixStream};

const AF_UNIX: u8 = 1;
const SOCK_STREAM: i32 = 1;
const SUN_LEN: usize = 104;

#[repr(C)]
struct LocalAddress {
    length: u8,
    family: u8,
    path: [u8; SUN_LEN],
}

extern "C" {
    fn pthread_fchdir_np(fd: i32) -> i32;
    fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
    fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
    #[link_name = "connect"]
    fn connect_socket(fd: i32, address: *const LocalAddress, length: u32) -> i32;
}

pub(super) fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    // The service authorizes by UID. LOCAL_PEEREPID is not available on every
    // Darwin UNIX connection (notably socketpair).
    if unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

fn in_held_directory<T>(directory: &File, action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    struct ResetCwd;
    impl Drop for ResetCwd {
        fn drop(&mut self) {
            // -1 clears this thread's override. The process cwd never changes.
            unsafe { pthread_fchdir_np(-1) };
        }
    }
    if unsafe { pthread_fchdir_np(directory.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let _reset = ResetCwd;
    // Deliberately synchronous. Never yield while the thread override is set.
    action()
}

pub(super) fn bind(directory: &File, leaf: &OsStr) -> io::Result<UnixListener> {
    let listener = in_held_directory(directory, || unix_net::UnixListener::bind(Path::new(leaf)))?;
    listener.set_nonblocking(true)?;
    UnixListener::from_std(listener)
}

pub(super) async fn connect(directory: &File, leaf: &OsStr) -> io::Result<UnixStream> {
    let name = leaf.as_bytes();
    if name.is_empty() || name.len() >= SUN_LEN || name.contains(&0) || name.contains(&b'/') {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut address = LocalAddress {
        length: (2 + name.len() + 1) as u8,
        family: AF_UNIX,
        path: [0; SUN_LEN],
    };
    address.path[..name.len()].copy_from_slice(name);
    let fd = unsafe { socket(i32::from(AF_UNIX), SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
    let socket = unix_net::UnixStream::from(fd);
    socket.set_nonblocking(true)?;
    in_held_directory(directory, || {
        // The C sockaddr only contains the relative leaf. Lookup uses the
        // exact held directory, and socket completion happens after cwd reset.
        if unsafe { connect_socket(socket.as_raw_fd(), &address, u32::from(address.length)) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(rustix::io::Errno::INPROGRESS.raw_os_error()) {
                return Err(error);
            }
        }
        Ok(())
    })?;
    let stream = UnixStream::from_std(socket)?;
    stream.writable().await?;
    if let Some(error) = stream.take_error()? {
        return Err(error);
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{connect_verified, Listener};
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt};

    #[tokio::test(flavor = "current_thread")]
    async fn long_private_socket_uses_held_parent_on_both_sides() -> anyhow::Result<()> {
        let scratch = crate::tests::scratch();
        let directory = scratch.path().join("long-private-data-".repeat(8));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let original = File::open(&directory)?;
        let mut listener = Listener::bind_verified(&directory, original.try_clone()?)?;
        let client = connect_verified(&listener.address(), &directory, &original).await?;
        let server = listener.accept().await?;
        client.verify_peer()?;
        server.verify_peer()?;
        assert!(std::fs::symlink_metadata(listener.address())?
            .file_type()
            .is_socket());
        Ok(())
    }
}
