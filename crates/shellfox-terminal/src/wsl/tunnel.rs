use super::{
    protocol::{read_frame, write_frame, Frame, CREDIT_BYTES},
    WslError,
};
use crate::PtyOutput;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct Tunnel {
    child: Child,
    writer: Option<SyncSender<Value>>,
    pub frames: Receiver<Result<Frame, WslError>>,
    fault: SyncSender<Result<Frame, WslError>>,
    output: Receiver<PtyOutput>,
    pending: Arc<AtomicUsize>,
    output_complete: Arc<AtomicBool>,
    pending_acks: Mutex<VecDeque<usize>>,
    guest_possible: bool,
    guest_zero: bool,
    #[cfg(windows)]
    job: Option<super::windows::TransportJob>,
}
impl Tunnel {
    pub fn spawn(mut command: Command) -> Result<Self, WslError> {
        #[cfg(windows)]
        let job = super::windows::TransportJob::new()?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|_| WslError::Unavailable)?;
        #[cfg(windows)]
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let setup = (|| {
            let stdin = child.stdin.take().ok_or(WslError::Io)?;
            let stdout = child.stdout.take().ok_or(WslError::Io)?;
            let mut stderr = child.stderr.take().ok_or(WslError::Io)?;
            let (writer, incoming) = mpsc::sync_channel::<Value>(32);
            let (control, frames) = mpsc::sync_channel(8);
            let (data, output) = mpsc::sync_channel(64);
            let pending = Arc::new(AtomicUsize::new(0));
            let send_control = control.clone();
            let fault = control.clone();
            thread::Builder::new()
                .name("wsl-control-writer".into())
                .spawn(move || {
                    let mut stdin = stdin;
                    while let Ok(frame) = incoming.recv() {
                        if write_frame(&mut stdin, &frame).is_err() {
                            let _ = send_control.try_send(Err(WslError::Io));
                            break;
                        }
                    }
                })
                .map_err(|_| WslError::Unavailable)?;
            let input_pending = pending.clone();
            let output_complete = Arc::new(AtomicBool::new(false));
            let completed = output_complete.clone();
            thread::Builder::new()
                .name("wsl-frame-reader".into())
                .spawn(move || read_frames(stdout, control, data, input_pending, completed))
                .map_err(|_| WslError::Unavailable)?;
            thread::Builder::new()
                .name("wsl-redacted-stderr".into())
                .spawn(move || {
                    let mut scratch = [0; 4096];
                    while let Ok(size) = stderr.read(&mut scratch) {
                        if size == 0 {
                            break;
                        }
                    }
                })
                .map_err(|_| WslError::Unavailable)?;
            Ok((writer, frames, fault, output, pending, output_complete))
        })();
        let (writer, frames, fault, output, pending, output_complete) = match setup {
            Ok(value) => value,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            writer: Some(writer),
            frames,
            fault,
            output,
            pending,
            output_complete,
            pending_acks: Mutex::new(VecDeque::new()),
            guest_possible: false,
            guest_zero: false,
            #[cfg(windows)]
            job: Some(job),
        })
    }
    pub fn mark_launch(&mut self) {
        self.guest_possible = true;
    }
    pub fn confirm_zero(&mut self) {
        self.guest_zero = true;
    }
    pub fn send(&self, value: Value) -> Result<(), WslError> {
        self.writer
            .as_ref()
            .ok_or(WslError::Io)?
            .try_send(value)
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => WslError::Busy,
                _ => WslError::Io,
            })
    }
    pub fn input(&self, bytes: &[u8]) -> Result<(), WslError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > CREDIT_BYTES {
            return Err(WslError::InvalidRequest);
        }
        self.pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes.len())
                    .filter(|n| *n <= CREDIT_BYTES)
            })
            .map_err(|_| WslError::Busy)?;
        let result = self.send(serde_json::json!({"action":"input","data":STANDARD.encode(bytes)}));
        if result.is_err() {
            self.pending.fetch_sub(bytes.len(), Ordering::AcqRel);
        }
        result
    }
    pub fn read(&self) -> Result<Option<PtyOutput>, TryRecvError> {
        let _ = self.flush_acks();
        match self.output.try_recv() {
            Ok(PtyOutput::Data(bytes)) => {
                if let Ok(mut pending) = self.pending_acks.lock() {
                    if pending.len() < 32 {
                        pending.push_back(bytes.len());
                    } else {
                        let _ = self.fault.try_send(Err(WslError::Protocol));
                    }
                }
                // Never discard received terminal bytes because a credit races
                // EOF or a full control queue. Credits retry only before dispatch.
                let _ = self.flush_acks();
                Ok(Some(PtyOutput::Data(bytes)))
            }
            Ok(item) => Ok(Some(item)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub fn flush_acks(&self) -> Result<(), WslError> {
        let mut pending = self.pending_acks.lock().map_err(|_| WslError::Io)?;
        if self.output_complete.load(Ordering::Acquire) {
            pending.clear();
            return Ok(());
        }
        while let Some(bytes) = pending.front().copied() {
            match self.send(serde_json::json!({"action":"ack","bytes":bytes})) {
                Ok(()) => {
                    pending.pop_front();
                }
                Err(WslError::Busy) => break,
                Err(error) => {
                    let _ = self.fault.try_send(Err(error));
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub fn next(&self) -> Result<Frame, WslError> {
        self.next_until(Instant::now() + Duration::from_secs(8))
    }
    pub fn next_until(&self, deadline: Instant) -> Result<Frame, WslError> {
        self.frames
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| WslError::Deadline)?
    }
    pub fn finish(&mut self) -> Result<(), WslError> {
        if self.guest_possible && !self.guest_zero {
            return Err(WslError::CleanupUnconfirmed);
        }
        self.writer.take();
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.finish()?;
        }
        #[cfg(not(windows))]
        {
            let _ = self.child.kill();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.child.try_wait().map_err(|_| WslError::Io)?.is_none() {
            if Instant::now() >= deadline {
                return Err(WslError::Deadline);
            }
            thread::sleep(Duration::from_millis(5));
        }
        #[cfg(windows)]
        self.job.take();
        Ok(())
    }
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        if self.guest_possible && !self.guest_zero {
            // EOF asks the guest to close. Never destroy its Windows tunnel as
            // though that were Linux cleanup proof. Session/reaper retain evidence.
            self.writer.take();
            #[cfg(windows)]
            if let Some(job) = self.job.take() {
                std::mem::forget(job);
            }
        } else {
            let _ = self.finish();
        }
    }
}
fn read_frames(
    mut stdout: impl Read,
    control: SyncSender<Result<Frame, WslError>>,
    data: SyncSender<PtyOutput>,
    pending: Arc<AtomicUsize>,
    output_complete: Arc<AtomicBool>,
) {
    let mut sequence = 0;
    loop {
        let frame = match read_frame(&mut stdout) {
            Ok(frame) => frame,
            Err(error) => {
                let _ = control.try_send(Err(error));
                let _ = data.try_send(PtyOutput::ReadError(std::io::Error::other(
                    "WSL frame channel stopped",
                )));
                break;
            }
        };
        let result = match frame {
            Frame::Data {
                sequence: next,
                data: encoded,
            } => Frame::data(next, &encoded, &mut sequence).and_then(|bytes| {
                data.try_send(PtyOutput::Data(bytes))
                    .map_err(|_| WslError::Protocol)
            }),
            Frame::InputAck { bytes } => {
                if bytes == 0 || bytes > super::protocol::DATA_BYTES {
                    Err(WslError::Protocol)
                } else {
                    pending
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                            n.checked_sub(bytes)
                        })
                        .map(|_| ())
                        .map_err(|_| WslError::Protocol)
                }
            }
            Frame::Eof {} => {
                output_complete.store(true, Ordering::Release);
                let _ = data.try_send(PtyOutput::Eof);
                let _ = control.try_send(Ok(Frame::Eof {}));
                break;
            }
            Frame::Error { code } => {
                let _ = code;
                Err(WslError::Unavailable)
            }
            frame => control.try_send(Ok(frame)).map_err(|_| WslError::Protocol),
        };
        if let Err(error) = result {
            let _ = control.try_send(Err(error));
            break;
        }
    }
}
