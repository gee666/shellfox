use crate::{process::Process, HelperError, IsolatedStateDirectory, VerifiedHelper};
use std::{sync::mpsc, thread, time::Duration};

/// Prestarted before a network helper can exist. Keeps the original child and
/// state capability even if its caller or Tokio runtime disappears on failure.
#[derive(Clone)]
pub(crate) struct CleanupOwner(mpsc::SyncSender<(Process, Option<IsolatedStateDirectory>)>);

pub(crate) struct PendingHelper {
    process: Option<Process>,
    state: Option<IsolatedStateDirectory>,
    cleanup: CleanupOwner,
}
impl PendingHelper {
    pub fn probe(executable: &VerifiedHelper) -> Result<Self, HelperError> {
        let cleanup = CleanupOwner::start()?;
        let process = Process::spawn(executable, true)?;
        Ok(Self {
            process: Some(process),
            state: None,
            cleanup,
        })
    }
    pub fn start(
        executable: &VerifiedHelper,
        state: IsolatedStateDirectory,
    ) -> Result<Self, HelperError> {
        let cleanup = CleanupOwner::start()?;
        let process = Process::spawn(executable, false)?;
        Ok(Self {
            process: Some(process),
            state: Some(state),
            cleanup,
        })
    }
    pub fn process(&mut self) -> Result<&mut Process, HelperError> {
        self.process.as_mut().ok_or(HelperError::Stopped)
    }
    pub fn finish(
        mut self,
    ) -> Result<(Process, IsolatedStateDirectory, CleanupOwner), HelperError> {
        // Check/take state before moving the original child. An erroneous
        // probe-to-start transition still drops through retaining cleanup.
        let state = self.state.take().ok_or(HelperError::Stopped)?;
        let process = self.process.take().ok_or(HelperError::Stopped)?;
        Ok((process, state, self.cleanup.clone()))
    }
}
impl Drop for PendingHelper {
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            self.cleanup.retain(process, self.state.take());
        }
    }
}
impl CleanupOwner {
    pub fn start() -> Result<Self, HelperError> {
        let (send, receive) = mpsc::sync_channel::<(Process, Option<IsolatedStateDirectory>)>(1);
        thread::Builder::new()
            .name("shellfox-go-cleanup".into())
            .spawn(move || {
                if let Ok((mut process, _state)) = receive.recv() {
                    loop {
                        if process.poll_exit() == Ok(true) {
                            break;
                        }
                        let _ = process.request_termination();
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            })
            .map_err(|_| HelperError::Spawn)?;
        Ok(Self(send))
    }
    pub fn retain(&self, process: Process, state: Option<IsolatedStateDirectory>) {
        if let Err(mpsc::SendError(retained)) = self.0.send((process, state)) {
            // A crashed cleanup owner must not destroy unconfirmed ownership.
            std::mem::forget(retained);
        }
    }
}
