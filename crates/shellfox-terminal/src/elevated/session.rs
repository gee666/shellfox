use super::stdio::{Observation, StdioIo};
use super::{AdapterError, Channel, Event, Generation, Phase, PreparedLaunch, ProcessIdentity};
use std::{
    process::Command,
    sync::mpsc::{self, SyncSender},
    time::Duration,
};

/// Owns the original launcher, private streams and resource lease through the
/// authenticated elevated peer's final exit, not merely launcher termination.
pub struct Session {
    channel: Option<Channel<StdioIo>>,
    recovery: SyncSender<Channel<StdioIo>>,
    observation: Observation,
}
pub struct StartError {
    pub error: AdapterError,
    pub retained: Option<Box<Session>>,
}
impl std::fmt::Debug for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.error, f)
    }
}
impl Session {
    pub(super) fn start(
        command: Command,
        hold: Box<dyn Send>,
        generation: Generation,
        launch: PreparedLaunch,
    ) -> Result<Self, StartError> {
        // No fallible cleanup-owner creation after a launch frame can reach UAC.
        let recovery = recovery_owner().map_err(|_| StartError {
            error: AdapterError::Transport,
            retained: None,
        })?;
        let io = StdioIo::spawn(command, hold).map_err(|failure| {
            // Dropping only a pre-launch owner enters its original-handle
            // quarantine. No shell or elevated peer has been released.
            drop(failure.retained);
            StartError {
                error: failure.error,
                retained: None,
            }
        })?;
        let observation = io.observation();
        match Channel::start(io, generation, launch) {
            Ok(channel) => Ok(Self {
                channel: Some(channel),
                recovery,
                observation,
            }),
            Err(failure) => Err(StartError {
                error: failure.error,
                retained: Some(Box::new(Self {
                    channel: Some(*failure.retained),
                    recovery,
                    observation,
                })),
            }),
        }
    }
    pub fn service(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .service(generation)
    }
    pub fn observation(&self) -> Observation {
        self.observation.clone()
    }
    pub fn phase(&self) -> Phase {
        self.channel
            .as_ref()
            .map(Channel::phase)
            .unwrap_or(Phase::Closed)
    }
    pub fn identity(&self) -> Option<&ProcessIdentity> {
        self.channel.as_ref().and_then(Channel::identity)
    }
    pub fn cleanup_confirmed(&self) -> bool {
        self.channel
            .as_ref()
            .is_some_and(Channel::cleanup_confirmed)
            && !self.observation.resources_held()
    }
    pub fn queued_input_bytes(&self) -> Option<usize> {
        self.channel.as_ref().and_then(Channel::queued_input_bytes)
    }
    pub fn startup_timed_out(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .startup_timed_out(generation)
    }
    pub fn close_timed_out(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .close_timed_out(generation)
    }
    pub fn write(&mut self, generation: Generation, bytes: &[u8]) -> Result<u64, AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::InputUnavailable)?
            .write(generation, bytes)
    }
    pub fn resize(
        &mut self,
        generation: Generation,
        cols: u16,
        rows: u16,
    ) -> Result<(), AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::InputUnavailable)?
            .resize(generation, cols, rows)
    }
    pub fn poll(&mut self, generation: Generation) -> Result<Option<Event>, AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .poll(generation)
    }
    pub fn begin_close(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .begin_close(generation)
    }
    /// Call only after draining data through final exit. Timeout never releases
    /// the bundle or turns native launcher exit into elevated peer proof.
    pub fn finish(&mut self, generation: Generation, budget: Duration) -> Result<(), AdapterError> {
        self.channel
            .as_mut()
            .ok_or(AdapterError::CleanupUnconfirmed)?
            .finish(generation, budget)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let Some(mut channel) = self.channel.take() else {
            return;
        };
        let generation = channel.generation();
        if channel.cleanup_confirmed() && channel.finish(generation, Duration::ZERO).is_ok() {
            return;
        }
        let _ = channel.begin_close(generation);
        // A disconnected recovery owner cannot silently unlock the bundle.
        // StdioIo's fallback retains original handles and its complete hold.
        let _ = self.recovery.send(channel);
    }
}
fn recovery_owner() -> std::io::Result<SyncSender<Channel<StdioIo>>> {
    let (send, receive) = mpsc::sync_channel::<Channel<StdioIo>>(1);
    std::thread::Builder::new()
        .name("elevated-cleanup-owner".into())
        .spawn(move || {
            let Ok(mut channel) = receive.recv() else {
                return;
            };
            let generation = channel.generation();
            loop {
                let _ = channel.begin_close(generation);
                // Bound each batch, discard retired bytes and keep ownership. This
                // is cleanup, never a second terminal parser or query responder.
                for _ in 0..64 {
                    match channel.poll(generation) {
                        Ok(Some(_)) => {}
                        _ => break,
                    }
                }
                if channel.cleanup_confirmed()
                    && channel
                        .finish(generation, Duration::from_millis(50))
                        .is_ok()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })?;
    Ok(send)
}
