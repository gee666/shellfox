use anyhow::{bail, ensure, Context, Result};
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

const MAX_WRITE_BYTES: usize = 64 * 1024;
/// Writes queued behind the one the shell is reading, at most.
const QUEUED_WRITES: usize = 4;

pub(crate) struct InputWriter {
    tx: Option<SyncSender<Vec<u8>>>,
    /// The error that stopped the writer thread, reported by the next write.
    failure: Arc<Mutex<Option<io::Error>>>,
    thread: Option<JoinHandle<()>>,
}

impl InputWriter {
    pub fn start(mut writer: Box<dyn Write + Send>) -> io::Result<Self> {
        let (tx, rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::sync_channel(QUEUED_WRITES);
        let failure = Arc::new(Mutex::new(None));
        let failed = failure.clone();
        let thread = thread::Builder::new()
            .name("shellfox-pty-write".into())
            .spawn(move || {
                while let Ok(bytes) = rx.recv() {
                    if let Err(error) = writer.write_all(&bytes) {
                        *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
                        break;
                    }
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            failure,
            thread: Some(thread),
        })
    }

    /// Queues input, written in order on the writer thread; the caller never
    /// waits for the shell to read it. A shell that reads slowly is
    /// backpressure, not a failure: with the queue full, new input fails with
    /// `InputBusy`, unwritten. A failed write ends input; the next call reports it.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= MAX_WRITE_BYTES, "PTY write exceeds 64 KiB");
        if let Some(error) = self
            .failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            self.tx.take();
            return Err(error).context("write PTY input");
        }
        let sender = self.tx.as_ref().context("PTY input closed")?;
        match sender.try_send(bytes.to_vec()) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(crate::InputBusy.into()),
            Err(TrySendError::Disconnected(_)) => bail!("PTY input closed"),
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
    use std::time::{Duration, Instant};

    /// A shell that reads nothing until `open` is sent, then everything.
    struct Stalled {
        started: mpsc::Sender<()>,
        open: Receiver<()>,
        opened: bool,
        written: Arc<Mutex<Vec<u8>>>,
    }
    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.opened {
                let _ = self.started.send(());
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
        let (started, reading) = mpsc::channel();
        let (open, gate) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let mut writer = InputWriter::start(Box::new(Stalled {
            started,
            open: gate,
            opened: false,
            written: written.clone(),
        }))
        .unwrap();
        writer.write(b"a").unwrap();
        reading.recv().unwrap();
        // The shell is stuck on "a"; the queue takes the next writes at once.
        for _ in 0..QUEUED_WRITES {
            writer.write(b"q").unwrap();
        }
        let busy = writer.write(b"x").unwrap_err();
        assert!(busy.is::<crate::InputBusy>());
        open.send(()).unwrap();
        // Once the shell reads, the queue drains in order and input flows again.
        let deadline = Instant::now() + Duration::from_secs(5);
        while written.lock().unwrap().len() < 1 + QUEUED_WRITES && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        writer.write(b"d").unwrap();
        writer.stop();
        writer.join().unwrap();
        assert_eq!(&*written.lock().unwrap(), b"aqqqqd");
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_write_ends_input_and_the_next_call_reports_it() {
        let mut writer = InputWriter::start(Box::new(Broken)).unwrap();
        writer.write(b"a").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !writer.finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let error = writer.write(b"b").unwrap_err();
        assert!(!error.is::<crate::InputBusy>());
        assert!(writer.write(b"c").is_err());
        writer.join().unwrap();
    }
}
