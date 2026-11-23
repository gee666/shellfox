use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Stop {
    pub cancel: AtomicBool,
    pub drain: AtomicBool,
}

pub(super) struct PtyIo {
    file: File,
    stop: Arc<Stop>,
}

impl PtyIo {
    pub fn pair(fd: RawFd, stop: Arc<Stop>) -> io::Result<(Self, Self)> {
        // portable-pty owns the master; each worker keeps its own descriptor.
        let file = File::from(unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()?);
        let reader = Self::new(file.try_clone()?, Arc::clone(&stop))?;
        Ok((reader, Self { file, stop }))
    }

    /// Also used for raw pipes, which an escaped descendant can hold open.
    pub fn new(file: File, stop: Arc<Stop>) -> io::Result<Self> {
        let fd = file.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { file, stop })
    }

    fn ready(&self, events: libc::c_short) -> io::Result<bool> {
        let mut fd = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events,
            revents: 0,
        };
        while !self.stop.cancel.load(Ordering::Acquire) {
            // An escaped descendant can keep the slave open. Never let that
            // keep a reader/writer thread alive after we close this terminal.
            let result = unsafe { libc::poll(&mut fd, 1, 25) };
            if result > 0 {
                return Ok(!self.stop.cancel.load(Ordering::Acquire));
            }
            if result == 0 && self.stop.drain.load(Ordering::Acquire) {
                return Ok(false);
            }
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
        Ok(false)
    }
}

impl Read for PtyIo {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        while self.ready(libc::POLLIN)? {
            match self.file.read(bytes) {
                Err(error) if error.raw_os_error() == Some(libc::EIO) => return Ok(0),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => return result,
            }
        }
        Ok(0)
    }
}

impl Write for PtyIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        while self.ready(libc::POLLOUT)? {
            match self.file.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => return result,
            }
        }
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
