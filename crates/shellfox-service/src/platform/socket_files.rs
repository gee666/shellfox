//! Unix socket files of the local endpoint, `<uuid>.sock` in the host's
//! private directory. Every lookup and unlink goes through the held directory
//! handle and never follows a link, so a swapped path cannot redirect them.
use anyhow::{ensure, Result};
use rustix::fs::{statat, unlinkat, AtFlags, Dir, FileType};
use std::{ffi::CStr, os::fd::AsFd};

/// Device and inode of the socket this host bound.
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

/// Unlinks the socket file only while it is still the one this host bound.
pub(super) fn remove(directory: impl AsFd, leaf: &CStr, bound: SocketIdentity) -> Result<()> {
    let directory = directory.as_fd();
    ensure!(
        identity(directory, leaf)? == bound,
        "The endpoint socket file was replaced"
    );
    unlinkat(directory, leaf, AtFlags::empty())?;
    Ok(())
}

/// Unlinks the sockets earlier hosts left behind. Call only while holding the
/// host ownership of this directory: then no other host listens on them.
pub(crate) fn remove_stale(directory: &std::fs::File) -> Result<usize> {
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

    #[test]
    fn stale_sockets_go_and_everything_else_stays() -> Result<()> {
        let scratch = crate::tests::scratch();
        let directory = scratch.path().join("sockets");
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let stale = format!("{}.sock", uuid::Uuid::new_v4());
        let _old = std::os::unix::net::UnixListener::bind(directory.join(&stale))?;
        std::fs::write(directory.join(format!("{}.sock", uuid::Uuid::new_v4())), "")?;
        std::fs::write(directory.join("endpoint.json"), "{}")?;
        let outside = scratch.path().join("outside.sock");
        let _outside = std::os::unix::net::UnixListener::bind(&outside)?;
        std::os::unix::fs::symlink(
            &outside,
            directory.join(format!("{}.sock", uuid::Uuid::new_v4())),
        )?;
        let held = std::fs::File::open(&directory)?;
        assert_eq!(remove_stale(&held)?, 1);
        assert!(!directory.join(&stale).exists());
        assert!(outside.exists());
        assert_eq!(std::fs::read_dir(&directory)?.count(), 3);
        Ok(())
    }

    #[test]
    fn a_replaced_socket_file_is_kept() -> Result<()> {
        let scratch = crate::tests::scratch();
        let directory = scratch.path().join("sockets");
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let leaf = c"0.sock";
        let path = directory.join("0.sock");
        let held = std::fs::File::open(&directory)?;
        let first = std::os::unix::net::UnixListener::bind(&path)?;
        let bound = identity(&held, leaf)?;
        drop(first);
        std::fs::remove_file(&path)?;
        let _second = std::os::unix::net::UnixListener::bind(&path)?;
        assert!(remove(&held, leaf, bound).is_err());
        assert!(path.exists());
        let current = identity(&held, leaf)?;
        remove(&held, leaf, current)?;
        assert!(!path.exists());
        Ok(())
    }
}
