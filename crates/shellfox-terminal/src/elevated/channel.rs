use super::launch::dimensions;
use super::wire::{
    decode, encode, AdapterError, Message, Outgoing, ProcessIdentity, MAX_ID, MAX_INPUT,
    MAX_PENDING, MAX_WRITE,
};
use super::{HelperFrame, HelperIo, PreparedLaunch};
use std::{collections::VecDeque, num::NonZeroU64, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(NonZeroU64);
impl Generation {
    pub fn new(value: u64) -> Result<Self, AdapterError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(AdapterError::StaleGeneration)
    }
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Active,
    Closing,
    Unconfirmed,
    Closed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Ready(ProcessIdentity),
    LateReady(ProcessIdentity),
    Data(Vec<u8>),
    InputAcknowledged(u64),
    Failed(AdapterError),
    ExitConfirmed(i32),
}

/// Every method retains the transport, including all error and close paths.
/// Keep this controller until cleanup is confirmed. Launcher death is not proof.
pub struct Channel<T: HelperIo> {
    io: T,
    generation: Generation,
    phase: Phase,
    identity: Option<ProcessIdentity>,
    pending: VecDeque<(u64, usize)>,
    queued: usize,
    next_id: u64,
    failure: Option<AdapterError>,
    receive_failed: bool,
    input_closed: bool,
    close_sent: bool,
}
pub struct StartupError<T: HelperIo> {
    pub error: AdapterError,
    pub retained: Box<Channel<T>>,
}
impl<T: HelperIo> std::fmt::Debug for StartupError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.error, f)
    }
}
impl<T: HelperIo> std::fmt::Debug for Channel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ElevatedChannel { private transport }")
    }
}
impl<T: HelperIo> Channel<T> {
    pub fn start(
        io: T,
        generation: Generation,
        launch: PreparedLaunch,
    ) -> Result<Self, StartupError<T>> {
        let mut channel = Self {
            io,
            generation,
            phase: Phase::Starting,
            identity: None,
            pending: VecDeque::new(),
            queued: 0,
            next_id: 0,
            failure: None,
            receive_failed: false,
            input_closed: false,
            close_sent: false,
        };
        channel.io.launch_started();
        if channel.io.send_line(&launch.bytes).is_err() {
            channel.fail(AdapterError::Transport, false);
            channel.close_input();
            return Err(StartupError {
                error: AdapterError::Transport,
                retained: Box::new(channel),
            });
        }
        Ok(channel)
    }
    pub fn generation(&self) -> Generation {
        self.generation
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn identity(&self) -> Option<&ProcessIdentity> {
        self.identity.as_ref()
    }
    pub fn cleanup_confirmed(&self) -> bool {
        self.phase == Phase::Closed
    }
    pub fn queued_input_bytes(&self) -> Option<usize> {
        (self.phase == Phase::Active && self.failure.is_none()).then_some(self.queued)
    }
    pub fn failure(&self) -> Option<AdapterError> {
        self.failure
    }
    fn current(&self, generation: Generation) -> Result<(), AdapterError> {
        if generation != self.generation {
            return Err(AdapterError::StaleGeneration);
        }
        Ok(())
    }
    fn active(&self, generation: Generation) -> Result<(), AdapterError> {
        self.current(generation)?;
        if self.phase != Phase::Active || self.failure.is_some() {
            return Err(AdapterError::InputUnavailable);
        }
        Ok(())
    }
    pub fn write(&mut self, generation: Generation, data: &[u8]) -> Result<u64, AdapterError> {
        self.active(generation)?;
        if data.is_empty() || data.len() > MAX_WRITE || data.contains(&0) {
            return Err(AdapterError::InvalidInput);
        }
        let text = std::str::from_utf8(data).map_err(|_| AdapterError::InvalidInput)?;
        if self.pending.len() >= MAX_PENDING
            || self.queued + data.len() > MAX_INPUT
            || self.next_id == MAX_ID
        {
            return Err(AdapterError::QueueFull);
        }
        let id = self.next_id + 1;
        // Serialize before reserving an ID or input credit. Escaping can exceed
        // the frame cap even when the original UTF-8 write fits its byte limit.
        let frame = encode(&Outgoing::Write { id, data: text })?;
        self.pending.push_back((id, data.len()));
        self.queued += data.len();
        self.next_id = id;
        if let Err(error) = self.io.send_line(&frame) {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                self.pending.pop_back();
                self.queued -= data.len();
                self.next_id -= 1;
                return Err(AdapterError::QueueFull);
            }
            // Partial delivery is unknown. Never release credit for replay.
            self.fail(AdapterError::Transport, false);
            self.close_input();
            return Err(AdapterError::Transport);
        }
        Ok(id)
    }
    pub fn resize(
        &mut self,
        generation: Generation,
        cols: u16,
        rows: u16,
    ) -> Result<(), AdapterError> {
        self.active(generation)?;
        dimensions(cols, rows)?;
        if self.io.queued_write_bytes() > super::MAX_FRAME {
            return Err(AdapterError::QueueFull);
        }
        let frame = encode(&Outgoing::Resize { cols, rows })?;
        if let Err(error) = self.io.send_line(&frame) {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Err(AdapterError::QueueFull);
            }
            self.fail(AdapterError::Transport, false);
            self.close_input();
            return Err(AdapterError::Transport);
        }
        Ok(())
    }
    pub fn begin_close(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.current(generation)?;
        if self.phase == Phase::Closed || self.close_sent {
            return Ok(());
        }
        if self.input_closed {
            return Err(AdapterError::CleanupUnconfirmed);
        }
        self.close_sent = true;
        if self.failure.is_none() {
            self.phase = Phase::Closing;
        }
        if let Err(error) = self.io.send_line(&encode(&Outgoing::Close)?) {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                self.close_sent = false;
                return Err(AdapterError::QueueFull);
            }
            self.fail(AdapterError::Transport, false);
            self.close_input();
            return Err(AdapterError::CleanupUnconfirmed);
        }
        Ok(())
    }
    pub fn startup_timed_out(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.current(generation)?;
        if self.phase != Phase::Starting {
            return Ok(());
        }
        self.fail(AdapterError::Cancelled, false);
        self.begin_close(generation)?;
        Err(AdapterError::Cancelled)
    }
    pub fn close_timed_out(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.current(generation)?;
        if self.cleanup_confirmed() {
            return Ok(());
        }
        self.fail(AdapterError::CleanupUnconfirmed, false);
        Err(AdapterError::CleanupUnconfirmed)
    }
    pub fn service(&mut self, generation: Generation) -> Result<(), AdapterError> {
        self.current(generation)?;
        self.io.service()
    }
    pub fn finish(&mut self, generation: Generation, budget: Duration) -> Result<(), AdapterError> {
        self.current(generation)?;
        if !self.cleanup_confirmed() {
            return Err(AdapterError::CleanupUnconfirmed);
        }
        self.io.finish(budget)
    }
    pub fn poll(&mut self, generation: Generation) -> Result<Option<Event>, AdapterError> {
        self.current(generation)?;
        if self.receive_failed {
            return Err(AdapterError::CleanupUnconfirmed);
        }
        let frame = match self.io.try_receive_line() {
            Ok(None) => return Ok(None),
            Ok(Some(HelperFrame::Line(frame))) => frame,
            Ok(Some(HelperFrame::InputClosed)) => {
                if self.cleanup_confirmed() {
                    return Ok(None);
                }
                self.fail(AdapterError::Transport, false);
                self.close_input();
                return Ok(Some(Event::Failed(AdapterError::Transport)));
            }
            Ok(Some(HelperFrame::Eof)) => {
                if self.cleanup_confirmed() {
                    return Ok(None);
                }
                self.fail(AdapterError::CleanupUnconfirmed, true);
                return Err(AdapterError::CleanupUnconfirmed);
            }
            Err(_) => {
                self.fail(AdapterError::Transport, true);
                return Err(AdapterError::Transport);
            }
        };
        let message = match decode(&frame) {
            Ok(message) => message,
            Err(error) => {
                self.fail(error, true);
                return Err(error);
            }
        };
        match self.receive(message) {
            Ok(event) => Ok(Some(event)),
            Err(error) => {
                self.fail(error, true);
                Err(error)
            }
        }
    }
    fn receive(&mut self, message: Message) -> Result<Event, AdapterError> {
        if self.phase == Phase::Closed {
            return Err(AdapterError::InvalidProtocol);
        }
        Ok(match message {
            Message::Ready(identity) => {
                if self.identity.is_some() {
                    return Err(AdapterError::InvalidProtocol);
                }
                self.identity = Some(identity.clone());
                if self.phase == Phase::Starting && self.failure.is_none() {
                    self.phase = Phase::Active;
                    Event::Ready(identity)
                } else {
                    Event::LateReady(identity)
                }
            }
            Message::Data(data) => {
                if self.identity.is_none() {
                    return Err(AdapterError::InvalidProtocol);
                }
                Event::Data(data)
            }
            Message::Ack(id) => {
                let (expected, bytes) = self
                    .pending
                    .front()
                    .copied()
                    .ok_or(AdapterError::InvalidProtocol)?;
                if id != expected {
                    return Err(AdapterError::InvalidProtocol);
                }
                self.pending.pop_front();
                self.queued -= bytes;
                Event::InputAcknowledged(id)
            }
            Message::Error(error) => {
                self.fail(error, false);
                Event::Failed(error)
            }
            Message::Exit(code) => {
                self.phase = Phase::Closed;
                self.io.peer_exit_confirmed();
                self.pending.clear();
                self.queued = 0;
                Event::ExitConfirmed(code)
            }
        })
    }
    fn close_input(&mut self) {
        if !self.input_closed {
            self.input_closed = true;
            self.io.disconnect_input();
        }
    }
    fn fail(&mut self, error: AdapterError, disconnect: bool) {
        self.failure.get_or_insert(error);
        if self.phase != Phase::Closed {
            self.phase = Phase::Unconfirmed;
        }
        if disconnect {
            self.receive_failed = true;
            self.io.distrust_output();
            self.close_input();
        }
    }
}
