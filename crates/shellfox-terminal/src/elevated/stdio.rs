use super::stdio_workers::{QueuedWrite, ReadEvent, Shared, Workers};
use super::{AdapterError, HelperFrame, HelperIo, MAX_FRAME};
use std::{
    io,
    process::{Child, Command, Stdio},
    sync::{
        atomic::Ordering,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const MAX_WIRE_QUEUED: usize = 1024 * 1024;
#[derive(Clone)]
pub struct Observation(Arc<Shared>);
impl Observation {
    pub fn launcher_exited(&self) -> bool {
        self.0.launcher_gone.load(Ordering::Acquire)
    }
    pub fn peer_exit_confirmed(&self) -> bool {
        self.0.peer_gone.load(Ordering::Acquire)
    }
    pub fn resources_held(&self) -> bool {
        self.0.held.load(Ordering::Acquire)
    }
}

pub(super) struct StdioIo {
    child: Option<Child>,
    hold: Option<Box<dyn Send>>,
    state: Arc<Shared>,
    workers: Workers,
    frames: Receiver<ReadEvent>,
    writes: Option<SyncSender<QueuedWrite>>,
    peer_required: bool,
    write_failure_reported: bool,
    eof_reported: bool,
}
pub(super) struct SpawnError {
    pub error: AdapterError,
    pub retained: Option<Box<StdioIo>>,
}
impl StdioIo {
    /// Before a launch frame is sent this exact helper only waits on stdin.
    /// Every worker is installed before the caller can release that launch.
    pub fn spawn(mut command: Command, hold: Box<dyn Send>) -> Result<Self, SpawnError> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = command.spawn().map_err(|_| SpawnError {
            error: AdapterError::Transport,
            retained: None,
        })?;
        let state = Arc::new(Shared::default());
        state.held.store(true, Ordering::Release);
        let (send_frames, frames) = mpsc::sync_channel(64);
        let (writes, requests) = mpsc::sync_channel(128);
        let mut io = Self {
            child: Some(child),
            hold: Some(hold),
            state,
            workers: Workers::default(),
            frames,
            writes: Some(writes),
            peer_required: false,
            write_failure_reported: false,
            eof_reported: false,
        };
        let setup = (|| -> io::Result<()> {
            let child = io
                .child
                .as_mut()
                .ok_or_else(|| io::Error::other("launcher owner missing"))?;
            let input = child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("input pipe missing"))?;
            let output = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("output pipe missing"))?;
            let error = child
                .stderr
                .take()
                .ok_or_else(|| io::Error::other("diagnostic pipe missing"))?;
            io.workers
                .start_writer(input, requests, Arc::clone(&io.state))?;
            io.workers
                .start_reader(output, send_frames, Arc::clone(&io.state))?;
            io.workers.start_stderr(error)
        })();
        if setup.is_err() {
            return Err(SpawnError {
                error: AdapterError::Transport,
                retained: Some(Box::new(io)),
            });
        }
        Ok(io)
    }
    pub fn observation(&self) -> Observation {
        Observation(Arc::clone(&self.state))
    }
    pub fn poll_launcher(&mut self) -> Result<bool, AdapterError> {
        if self.state.launcher_gone.load(Ordering::Acquire) {
            return Ok(true);
        }
        if self
            .child
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .try_wait()
            .map_err(|_| AdapterError::Transport)?
            .is_some()
        {
            self.state.launcher_gone.store(true, Ordering::Release);
            return Ok(true);
        }
        Ok(false)
    }
}
impl HelperIo for StdioIo {
    fn send_line(&mut self, line: &[u8]) -> io::Result<()> {
        if line.is_empty() || line.len() > MAX_FRAME || line.last() != Some(&b'\n') {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        if self.state.cancel.load(Ordering::Acquire)
            || self.state.write_failed.load(Ordering::Acquire)
        {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        if self
            .state
            .queued
            .load(Ordering::Acquire)
            .saturating_add(line.len())
            > MAX_WIRE_QUEUED
        {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let request = QueuedWrite::new(line.to_vec(), Arc::clone(&self.state));
        match self
            .writes
            .as_ref()
            .ok_or(io::ErrorKind::BrokenPipe)?
            .try_send(request)
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(io::Error::from(io::ErrorKind::WouldBlock)),
            Err(TrySendError::Disconnected(_)) => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        }
    }
    fn try_receive_line(&mut self) -> io::Result<Option<HelperFrame>> {
        self.poll_launcher()
            .map_err(|_| io::Error::other("launcher observation failed"))?;
        if self.state.cancel.load(Ordering::Acquire) {
            self.workers.cancel_writer();
        }
        if self.state.write_failed.load(Ordering::Acquire) && !self.write_failure_reported {
            self.write_failure_reported = true;
            return Ok(Some(HelperFrame::InputClosed));
        }
        if self.eof_reported {
            return Ok(None);
        }
        match self.frames.try_recv() {
            Ok(ReadEvent::Frame(HelperFrame::Eof)) => {
                self.eof_reported = true;
                Ok(Some(HelperFrame::Eof))
            }
            Ok(ReadEvent::Frame(frame)) => Ok(Some(frame)),
            Ok(ReadEvent::Invalid) => Err(io::Error::from(io::ErrorKind::InvalidData)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        }
    }
    fn service(&mut self) -> Result<(), AdapterError> {
        self.poll_launcher().map(|_| ())
    }
    fn queued_write_bytes(&self) -> usize {
        self.state.queued.load(Ordering::Acquire)
    }
    fn disconnect_input(&mut self) {
        self.state.cancel.store(true, Ordering::Release);
        self.writes.take();
        self.workers.cancel_writer();
    }
    fn distrust_output(&mut self) {
        self.state.discard_output.store(true, Ordering::Release);
    }
    fn launch_started(&mut self) {
        self.peer_required = true;
    }
    fn peer_exit_confirmed(&mut self) {
        self.state.peer_gone.store(true, Ordering::Release);
    }
    fn finish(&mut self, budget: Duration) -> Result<(), AdapterError> {
        if self.hold.is_none() {
            return Ok(());
        }
        let deadline = Instant::now() + budget;
        loop {
            self.disconnect_input();
            let peer = !self.peer_required || self.state.peer_gone.load(Ordering::Acquire);
            if peer
                && self.poll_launcher()?
                && self.state.eof.load(Ordering::Acquire)
                && self.workers.finished()
            {
                self.workers.join()?;
                self.child.take();
                self.hold.take();
                self.state.held.store(false, Ordering::Release);
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(AdapterError::CleanupUnconfirmed);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

struct Retained {
    child: Child,
    _hold: Box<dyn Send>,
    workers: Workers,
    state: Arc<Shared>,
    peer_required: bool,
}
static RETAINED: OnceLock<Mutex<Vec<Retained>>> = OnceLock::new();
impl Drop for StdioIo {
    fn drop(&mut self) {
        self.disconnect_input();
        self.state.discard_output.store(true, Ordering::Release);
        let (Some(child), Some(hold)) = (self.child.take(), self.hold.take()) else {
            return;
        };
        // Last-resort retention if the prestarted protocol recovery owner failed.
        // It never infers peer death from launcher death, EOF or a timeout.
        RETAINED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Retained {
                child,
                _hold: hold,
                workers: std::mem::take(&mut self.workers),
                state: Arc::clone(&self.state),
                peer_required: self.peer_required,
            });
    }
}
/// Reap fallback owners only after original launcher AND required peer proof.
/// No process discovery or signaling occurs here.
pub fn reap_retained_launchers() -> usize {
    let mut owners = RETAINED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut reaped = 0;
    let mut index = 0;
    while index < owners.len() {
        let owner = &mut owners[index];
        let peer = !owner.peer_required || owner.state.peer_gone.load(Ordering::Acquire);
        if peer && matches!(owner.child.try_wait(), Ok(Some(_))) && owner.workers.finished() {
            owner.state.held.store(false, Ordering::Release);
            owners.swap_remove(index);
            reaped += 1;
        } else {
            index += 1;
        }
    }
    reaped
}
