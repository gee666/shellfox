//! Unix socket files of the local endpoint, `<uuid>.sock` in the host's
//! private directory. Every lookup, connect and unlink goes through the held
//! directory handle and never follows a link, so a swapped path cannot
//! redirect them.
use anyhow::{ensure, Result};
use rustix::fs::{statat, unlinkat, AtFlags, Dir, FileType};
use std::{ffi::CStr, fs::File, os::fd::AsFd, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

/// Device and inode of the socket this host bound. A file system may hand
/// a freed inode number to the next file at once, so this alone does not
/// prove the file is still ours; `remove` also probes the listener.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct SocketIdentity {
    // i128 holds every platform's dev_t and ino_t without a cast.
    dev: i128,
    ino: i128,
}

pub(super) fn identity(directory: impl AsFd, leaf: &CStr) -> Result<SocketIdentity> {
    let stat = statat(directory, leaf, AtFlags::SYMLINK_NOFOLLOW)?;
    ensure!(
        FileType::from_raw_mode(stat.st_mode) == FileType::Socket,
        "The endpoint is not a socket"
    );
    Ok(SocketIdentity {
        dev: i128::from(stat.st_dev),
        ino: i128::from(stat.st_ino),
    })
}

/// Unlinks the socket file only while it still leads to `listener`: a probe
/// connection made through the held directory must arrive there. Call
/// before closing the listener.
pub(super) async fn remove(
    listener: &UnixListener,
    directory: &File,
    leaf: &CStr,
    bound: SocketIdentity,
) -> Result<()> {
    ensure!(
        identity(directory, leaf)? == bound,
        "The endpoint socket file was replaced"
    );
    let token = *uuid::Uuid::new_v4().as_bytes();
    let mut probe = connect(directory, leaf).await?;
    probe.write_all(&token).await?;
    let arrived = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut received = [0; 16];
            let read =
                tokio::time::timeout(Duration::from_millis(500), stream.read_exact(&mut received))
                    .await;
            if matches!(read, Ok(Ok(_))) && received == token {
                return std::io::Result::Ok(());
            }
        }
    })
    .await;
    ensure!(
        matches!(arrived, Ok(Ok(()))),
        "The endpoint socket file no longer leads to this host"
    );
    unlinkat(directory, leaf, AtFlags::empty())?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
async fn connect(directory: &File, leaf: &CStr) -> std::io::Result<UnixStream> {
    use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
    // The held directory keeps the kernel path short and the lookup anchored.
    let path = std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .join(std::ffi::OsStr::from_bytes(leaf.to_bytes()));
    UnixStream::connect(path).await
}

#[cfg(target_os = "macos")]
async fn connect(directory: &File, leaf: &CStr) -> std::io::Result<UnixStream> {
    use std::os::unix::ffi::OsStrExt;
    super::darwin::connect(directory, std::ffi::OsStr::from_bytes(leaf.to_bytes())).await
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
async fn connect(_: &File, _: &CStr) -> std::io::Result<UnixStream> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Unlinks the sockets earlier hosts left behind. Call only while holding the
/// host ownership of this directory: then no other host listens on them.
pub(crate) fn remove_stale(directory: &File) -> Result<usize> {
    let owner = rustix::process::geteuid().as_raw();
    let mut removed = 0;
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        let leaf = entry.file_name();
        if !is_socket_name(leaf) {
            continue;
        }
        let Ok(stat) = statat(directory, leaf, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        if FileType::from_raw_mode(stat.st_mode) == FileType::Socket
            && stat.st_uid == owner
            && unlinkat(directory, leaf, AtFlags::empty()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_socket_name(leaf: &CStr) -> bool {
    leaf.to_str()
        .ok()
        .and_then(|name| name.strip_suffix(".sock"))
        .is_some_and(|stem| uuid::Uuid::try_parse(stem).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    /// Binds as the host does, through the held directory, since a scratch
    /// path in the project is longer than a sockaddr allows.
    fn bind(directory: &File, leaf: &str) -> UnixListener {
        #[cfg(target_os = "macos")]
        let listener = super::super::darwin::bind(directory, std::ffi::OsStr::new(leaf));
        #[cfg(not(target_os = "macos"))]
        let listener = {
            use std::os::fd::AsRawFd;
            UnixListener::bind(format!("/proc/self/fd/{}/{leaf}", directory.as_raw_fd()))
        };
        listener.unwrap()
    }

    fn private_dir(scratch: &tempfile::TempDir, name: &str) -> (std::path::PathBuf, File) {
        let directory = scratch.path().join(name);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let held = File::open(&directory).unwrap();
        (directory, held)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_sockets_go_and_everything_else_stays() -> Result<()> {
        let scratch = crate::tests::scratch();
        let (directory, held) = private_dir(&scratch, "sockets");
        let stale = format!("{}.sock", uuid::Uuid::new_v4());
        let _old = bind(&held, &stale);
        std::fs::write(directory.join(format!("{}.sock", uuid::Uuid::new_v4())), "")?;
        std::fs::write(directory.join("endpoint.json"), "{}")?;
        let (outside_dir, outside_held) = private_dir(&scratch, "outside");
        let _outside = bind(&outside_held, "o.sock");
        let outside = outside_dir.join("o.sock");
        let link = directory.join(format!("{}.sock", uuid::Uuid::new_v4()));
        std::os::unix::fs::symlink(&outside, link)?;
        assert_eq!(remove_stale(&held)?, 1);
        assert!(!directory.join(&stale).exists());
        assert!(std::fs::symlink_metadata(&outside).is_ok());
        assert_eq!(std::fs::read_dir(&directory)?.count(), 3);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_replaced_socket_file_is_kept() -> Result<()> {
        let scratch = crate::tests::scratch();
        let (directory, held) = private_dir(&scratch, "sockets");
        let leaf = c"0.sock";
        let path = directory.join("0.sock");
        let first = bind(&held, "0.sock");
        let bound = identity(&held, leaf)?;
        std::fs::remove_file(&path)?;
        // Often the same inode number as the first one.
        let second = bind(&held, "0.sock");
        assert!(remove(&first, &held, leaf, bound).await.is_err());
        assert!(std::fs::symlink_metadata(&path).is_ok());
        drop(second);
        assert!(remove(&first, &held, leaf, identity(&held, leaf)?)
            .await
            .is_err());
        assert!(std::fs::symlink_metadata(&path).is_ok());
        std::fs::remove_file(&path)?;
        let third = bind(&held, "0.sock");
        remove(&third, &held, leaf, identity(&held, leaf)?).await?;
        assert!(std::fs::symlink_metadata(&path).is_err());
        Ok(())
    }
}
