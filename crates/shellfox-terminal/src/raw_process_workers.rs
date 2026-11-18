use super::*;
use anyhow::ensure;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};
#[cfg(test)]
#[path = "raw_process_stdin_worker_tests.rs"]
mod tests;
const INPUT_QUEUE: usize = 8;
type Cancellation = Arc<Mutex<Option<OwnedHandle>>>;
struct CancelOnExit(Cancellation);
impl Drop for CancelOnExit {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
struct WriteRequest {
    bytes: Vec<u8>,
    eof: bool,
    reply: SyncSender<Result<()>>,
}
pub(crate) struct Input {
    sender: SyncSender<WriteRequest>,
    closed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    cancellation: Cancellation,
    failure: Arc<Mutex<Option<StdinFailure>>>,
}
impl Input {
    pub fn start(writer: std::process::ChildStdin) -> Result<Self> {
        use windows_sys::Win32::{
            Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS},
            System::Threading::GetCurrentProcess,
        };
        let process = unsafe { GetCurrentProcess() };
        let mut handle = std::ptr::null_mut();
        ensure!(
            unsafe {
                DuplicateHandle(
                    process,
                    writer.as_raw_handle().cast(),
                    process,
                    &mut handle,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            } != 0,
            "Automation stdin cancellation handle unavailable."
        );
        // This pins the same file object used by std's overlapped ChildStdin IO.
        // The worker closes BOTH handles before EOF acknowledgement.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
        let cancellation = Arc::new(Mutex::new(Some(handle)));
        let thread_cancel = cancellation.clone();
        let (sender, incoming) = mpsc::sync_channel::<WriteRequest>(INPUT_QUEUE);
        let closed = Arc::new(AtomicBool::new(false));
        let stopping = closed.clone();
        let failure = Arc::new(Mutex::new(None));
        let thread_failure = failure.clone();
        let thread = std::thread::Builder::new()
            .name("shellfox-raw-stdin".into())
            .spawn(move || {
                let cancel = CancelOnExit(thread_cancel);
                let mut writer = writer;
                while !stopping.load(Ordering::Acquire) {
                    let request = match incoming.recv_timeout(Duration::from_millis(20)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if stopping.load(Ordering::Acquire) {
                        let _ = request.reply.send(Err(terminal_error(&thread_failure)));
                        break;
                    }
                    let result = writer
                        .write_all(&request.bytes)
                        .map_err(StdinFailure::from_io);
                    if request.eof || result.is_err() {
                        let terminal = result
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(StdinFailure::known_closed);
                        thread_failure
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get_or_insert(terminal);
                        stopping.store(true, Ordering::Release);
                        drop(writer);
                        drop(cancel); // Both writer handles close before EOF ACK.
                        let _ = request.reply.send(result.map_err(StdinFailure::into_error));
                        return;
                    }
                    let _ = request.reply.send(result.map_err(StdinFailure::into_error));
                }
                stopping.store(true, Ordering::Release);
            })?;
        Ok(Self {
            sender,
            closed,
            thread: Some(thread),
            cancellation,
            failure,
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
            .try_send(WriteRequest { bytes, eof, reply })
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
        self.cancel();
    }
    pub fn cancel(&self) {
        #[cfg(windows)]
        if self.closed.load(Ordering::Acquire) {
            let handle = self.cancellation.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(handle) = handle.as_ref() {
                // ChildStdin uses overlapped IO. CancelSynchronousIo alone cannot
                // cancel it when a disconnected client's root remains alive.
                unsafe {
                    windows_sys::Win32::System::IO::CancelIoEx(
                        handle.as_raw_handle().cast(),
                        std::ptr::null(),
                    );
                }
            }
            if let Some(thread) = &self.thread {
                // The original join handle pins this writer thread. Repeated
                // cancellation closes the check/write race without a thread ID.
                unsafe {
                    windows_sys::Win32::System::IO::CancelSynchronousIo(
                        thread.as_raw_handle().cast(),
                    );
                }
            }
        }
    }
    pub fn join(&mut self) -> Result<bool> {
        self.cancel();
        if self.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return Ok(false);
        }
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("Automation stdin worker panicked."))?;
        }
        Ok(true)
    }
}
fn terminal_error(failure: &Mutex<Option<StdinFailure>>) -> anyhow::Error {
    failure
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .map(StdinFailure::into_error)
        .unwrap_or_else(|| AutomationStdinUnavailable::WriterDisconnected.into())
}
