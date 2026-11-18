use anyhow::{ensure, Context, Result};
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= MAX_WRITE_BYTES, "PTY write exceeds 64 KiB");
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .as_ref()
            .context("PTY input closed")?
            .try_send((bytes.to_vec(), tx))
            .map_err(|_| anyhow::anyhow!("PTY input worker unavailable or busy"))?;
        match rx.recv_timeout(WRITE_TIMEOUT) {
            Ok(result) => result.context("write PTY input"),
            Err(error) => {
                self.stop();
                Err(anyhow::anyhow!(
                    "PTY input failed or timed out; delivery may be partial: {error}"
                ))
            }
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
