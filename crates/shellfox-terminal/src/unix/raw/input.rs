use crate::raw_process::{io::StdinFailure, AutomationStdinUnavailable, WriteReceipt, INPUT_BYTES};
use anyhow::{ensure, Result};
use std::{
    io::Write,
    os::fd::AsRawFd,
    process::ChildStdin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};
struct Request {
    bytes: Vec<u8>,
    eof: bool,
    reply: SyncSender<Result<()>>,
}
pub(super) struct Input {
    sender: SyncSender<Request>,
    closed: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<StdinFailure>>>,
    thread: Option<JoinHandle<()>>,
}
impl Input {
    pub fn start(mut writer: ChildStdin) -> Result<Self> {
        let fd = writer.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        ensure!(
            flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
            "raw stdin nonblocking setup failed"
        );
        let (sender, incoming) = mpsc::sync_channel::<Request>(8);
        let closed = Arc::new(AtomicBool::new(false));
        let stopping = closed.clone();
        let failure = Arc::new(Mutex::new(None));
        let failed = failure.clone();
        let thread = std::thread::Builder::new()
            .name("linux-raw-stdin".into())
            .spawn(move || {
                loop {
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let request = match incoming.recv_timeout(Duration::from_millis(10)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let result = write_all(&mut writer, &request.bytes, &stopping)
                        .map_err(StdinFailure::from_io);
                    if request.eof || result.is_err() {
                        let terminal = result
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(StdinFailure::known_closed);
                        *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(terminal);
                        stopping.store(true, Ordering::Release);
                        drop(writer); // Actual OS EOF precedes the successful EOF ACK.
                        let _ = request.reply.send(result.map_err(StdinFailure::into_error));
                        return;
                    }
                    let _ = request.reply.send(Ok(()));
                }
                stopping.store(true, Ordering::Release);
            })?;
        Ok(Self {
            sender,
            closed,
            failure,
            thread: Some(thread),
        })
    }
    pub fn write(&self, bytes: Vec<u8>, eof: bool) -> Result<WriteReceipt> {
        ensure!(
            bytes.len() <= INPUT_BYTES,
            "Automation input exceeds its bound."
        );
        if let Some(failure) = self
            .failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(failure.into_error());
        }
        if self.closed.load(Ordering::Acquire) {
            return Err(AutomationStdinUnavailable::WriterDisconnected.into());
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.sender
            .try_send(Request { bytes, eof, reply })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => AutomationStdinUnavailable::QueueFull,
                mpsc::TrySendError::Disconnected(_) => {
                    AutomationStdinUnavailable::WriterDisconnected
                }
            })?;
        Ok(WriteReceipt { result })
    }
    pub fn close(&self) {
        self.failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(StdinFailure::known_closed);
        self.closed.store(true, Ordering::Release);
    }
    pub fn join(&mut self) -> Result<bool> {
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            return Ok(false);
        }
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("Raw stdin worker panicked."))?;
        }
        Ok(true)
    }
}
fn write_all(writer: &mut ChildStdin, bytes: &[u8], stopping: &AtomicBool) -> std::io::Result<()> {
    let mut position = 0;
    loop {
        if stopping.load(Ordering::Acquire) {
            // A partially delivered write remains an OS cancellation error, not
            // a successful zero-byte receipt or an inferred BrokenPipe.
            return Err(std::io::Error::from_raw_os_error(libc::ECANCELED));
        }
        if position == bytes.len() {
            return Ok(());
        }
        match writer.write(&bytes[position..]) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(count) => position += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let mut poll = libc::pollfd {
                    fd: writer.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                let result = unsafe { libc::poll(&mut poll, 1, 10) };
                if result < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() != std::io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
}
