use anyhow::{bail, ensure, Context, Result};
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// How long one write waits for the shell to read before it counts as queued.
const WRITE_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_WRITE_BYTES: usize = 64 * 1024;
type Request = (Vec<u8>, SyncSender<io::Result<()>>);

pub(crate) struct InputWriter {
    tx: Option<SyncSender<Request>>,
    thread: Option<JoinHandle<()>>,
}

impl InputWriter {
    pub fn start(mut writer: Box<dyn Write + Send>) -> io::Result<Self> {
        let (tx, rx): (SyncSender<Request>, Receiver<Request>) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("shellfox-pty-write".into())
            .spawn(move || {
                while let Ok((bytes, reply)) = rx.recv() {
                    let result = writer.write_all(&bytes);
                    let failed = result.is_err();
                    let _ = reply.send(result);
                    if failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    /// Writes in order. A shell that reads slowly is backpressure, not a failure:
    /// a write it hasn't read yet stays queued and is written once it reads, and
    /// while one more is already queued new input fails with `InputBusy`, unwritten.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= MAX_WRITE_BYTES, "PTY write exceeds 64 KiB");
        let sender = self.tx.as_ref().context("PTY input closed")?;
        let (reply, result) = mpsc::sync_channel(1);
        match sender.try_send((bytes.to_vec(), reply)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(crate::InputBusy.into()),
            Err(TrySendError::Disconnected(_)) => bail!("PTY input closed"),
        }
        match result.recv_timeout(WRITE_TIMEOUT) {
            Ok(result) => result.context("write PTY input"),
            Err(RecvTimeoutError::Timeout) => Ok(()),
            Err(RecvTimeoutError::Disconnected) => bail!("PTY input worker stopped"),
        }
    }

    pub fn stop(&mut self) {
        self.tx.take();
        #[cfg(windows)]
        if let Some(thread) = self.thread.as_ref() {
            use std::os::windows::io::AsRawHandle;
            // The live JoinHandle owns this exact thread handle. Failure can mean
            // no I/O was pending. Shutdown retries until the worker has exited.
            unsafe {
                windows_sys::Win32::System::IO::CancelSynchronousIo(thread.as_raw_handle().cast());
            }
        }
    }

    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn join(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("PTY input worker panicked"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    /// A shell that reads nothing until `open` is sent, then everything.
    struct Stalled {
        open: Receiver<()>,
        opened: bool,
        written: Arc<Mutex<Vec<u8>>>,
    }
    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.opened {
                let _ = self.open.recv();
                self.opened = true;
            }
            self.written.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_shell_that_reads_slowly_is_backpressure_not_a_failure() {
        let (open, gate) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut writer = InputWriter::start(Box::new(Stalled {
            open: gate,
            opened: false,
            written: written.clone(),
        }))
        .unwrap();
        // One write in progress and one queued count as accepted.
        writer.write(b"a").unwrap();
        writer.write(b"b").unwrap();
        let busy = writer.write(b"c").unwrap_err();
        assert!(busy.is::<crate::InputBusy>());
        open.send(()).unwrap();
        // Once the shell reads, the queue drains in order and input flows again.
        let deadline = Instant::now() + Duration::from_secs(5);
        while written.lock().unwrap().len() < 2 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        writer.write(b"d").unwrap();
        assert_eq!(&*written.lock().unwrap(), b"abd");
        writer.stop();
        writer.join().unwrap();
    }
}
