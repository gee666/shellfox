use super::{AdapterError, HelperFrame, LineDecoder};
use crate::wake::WakeSlot;
use std::{
    io::{Read, Write},
    process::{ChildStderr, ChildStdin, ChildStdout},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
};

#[derive(Default)]
pub(super) struct Shared {
    pub queued: AtomicUsize,
    pub cancel: AtomicBool,
    pub write_failed: AtomicBool,
    pub discard_output: AtomicBool,
    pub eof: AtomicBool,
    pub launcher_gone: AtomicBool,
    pub peer_gone: AtomicBool,
    pub held: AtomicBool,
    /// Pipe workers still running. Each one wakes the owner as it ends.
    running: AtomicUsize,
    pub wake: WakeSlot,
}
impl Shared {
    pub fn workers_finished(&self) -> bool {
        self.running.load(Ordering::Acquire) == 0
    }
}
/// Held by each worker thread: its end is the owner's cue to check cleanup.
struct Running(Arc<Shared>);
impl Running {
    fn new(state: &Arc<Shared>) -> Self {
        state.running.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(state))
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
        self.0.wake.wake();
    }
}
pub(super) enum ReadEvent {
    Frame(HelperFrame),
    Invalid,
}
pub(super) struct QueuedWrite {
    pub bytes: Vec<u8>,
    state: Arc<Shared>,
}
impl QueuedWrite {
    pub fn new(bytes: Vec<u8>, state: Arc<Shared>) -> Self {
        state.queued.fetch_add(bytes.len(), Ordering::AcqRel);
        Self { bytes, state }
    }
}
impl Drop for QueuedWrite {
    fn drop(&mut self) {
        self.state
            .queued
            .fetch_sub(self.bytes.len(), Ordering::AcqRel);
    }
}

fn publish(frames: &SyncSender<ReadEvent>, state: &Shared, mut event: ReadEvent) -> bool {
    loop {
        if state.discard_output.load(Ordering::Acquire) {
            return false;
        }
        match frames.try_send(event) {
            Ok(()) => {
                state.wake.wake();
                return true;
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return false,
            Err(std::sync::mpsc::TrySendError::Full(returned)) => event = returned,
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[derive(Default)]
pub(super) struct Workers {
    pub threads: Vec<JoinHandle<()>>,
    writer: Option<usize>,
}
impl Workers {
    pub fn start_writer(
        &mut self,
        mut input: ChildStdin,
        requests: Receiver<QueuedWrite>,
        state: Arc<Shared>,
    ) -> std::io::Result<()> {
        let running = Running::new(&state);
        let worker = std::thread::Builder::new()
            .name("elevated-input".into())
            .spawn(move || {
                let _running = running;
                for request in requests {
                    if state.cancel.load(Ordering::Acquire) {
                        break;
                    }
                    if input
                        .write_all(&request.bytes)
                        .and_then(|_| input.flush())
                        .is_err()
                    {
                        if !state.cancel.load(Ordering::Acquire) {
                            state.write_failed.store(true, Ordering::Release);
                        }
                        break;
                    }
                }
                // Drop stdin on every exit. The retained launcher then cancels its
                // private peer even when approval or native console work is pending.
            })?;
        self.writer = Some(self.threads.len());
        self.threads.push(worker);
        Ok(())
    }
    pub fn start_reader(
        &mut self,
        mut output: ChildStdout,
        frames: SyncSender<ReadEvent>,
        state: Arc<Shared>,
    ) -> std::io::Result<()> {
        let running = Running::new(&state);
        let worker = std::thread::Builder::new()
            .name("elevated-output".into())
            .spawn(move || {
                let _running = running;
                let mut decoder = LineDecoder::default();
                let mut buffer = [0u8; 4096];
                let mut listening = true;
                loop {
                    let count = match output.read(&mut buffer) {
                        Ok(0) => {
                            let result = decoder.finish(|frame| {
                                if listening {
                                    publish(&frames, &state, ReadEvent::Frame(frame));
                                }
                                Ok(())
                            });
                            if result.is_err() && listening {
                                publish(&frames, &state, ReadEvent::Invalid);
                            }
                            state.eof.store(true, Ordering::Release);
                            break;
                        }
                        Ok(count) => count,
                        Err(_) => {
                            publish(&frames, &state, ReadEvent::Invalid);
                            break;
                        }
                    };
                    if !listening || state.discard_output.load(Ordering::Acquire) {
                        listening = false;
                        continue;
                    }
                    let result = decoder.feed(&buffer[..count], |frame| {
                        if listening && !publish(&frames, &state, ReadEvent::Frame(frame)) {
                            listening = false;
                        }
                        Ok(())
                    });
                    if result.is_err() {
                        publish(&frames, &state, ReadEvent::Invalid);
                        listening = false;
                    }
                    // Keep draining if the consumer disappears or framing fails.
                    // Blocked stdout must not keep privileged cleanup alive.
                }
            })?;
        self.threads.push(worker);
        Ok(())
    }
    pub fn start_stderr(
        &mut self,
        mut error: ChildStderr,
        state: &Arc<Shared>,
    ) -> std::io::Result<()> {
        let running = Running::new(state);
        let worker = std::thread::Builder::new()
            .name("elevated-diagnostics".into())
            .spawn(move || {
                let _running = running;
                let mut buffer = [0u8; 4096];
                while matches!(error.read(&mut buffer), Ok(count) if count != 0) {}
            })?;
        self.threads.push(worker);
        Ok(())
    }
    pub fn cancel_writer(&self) {
        #[cfg(windows)]
        if let Some(worker) = self.writer.and_then(|index| self.threads.get(index)) {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: this is the original owned worker thread handle. No
            // numeric thread or process lookup and no handle ownership transfer.
            unsafe {
                windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle().cast());
            }
        }
    }
    pub fn join(&mut self) -> Result<(), AdapterError> {
        while let Some(thread) = self.threads.pop() {
            thread.join().map_err(|_| AdapterError::Transport)?;
        }
        Ok(())
    }
}
