//! The single authoritative screen source starts with the run, not with Show.
use shellfox_protocol::automation::{AutomationRun, AutomationStream, AUTOMATION_CREDIT};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use uuid::Uuid;

/// Attaches the run's ALREADY RUNNING external-source Runtime to a read-only tab.
/// This method must not create a parser from a replay tail or allocate a PTY.
pub trait AutomationViewer {
    fn create_read_only(&mut self, run: &AutomationRun) -> anyhow::Result<(Uuid, Uuid)>;
    fn focus(&mut self, session: Uuid) -> anyhow::Result<()>;
}
pub enum AutomationViewEvent {
    Data {
        sequence: u64,
        stream: AutomationStream,
        bytes: Vec<u8>,
    },
    Eof {
        after_sequence: u64,
        stream: AutomationStream,
    },
    Exit {
        after_sequence: u64,
        code: i32,
    },
}
impl std::fmt::Debug for AutomationViewEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data {
                sequence,
                stream,
                bytes,
            } => f
                .debug_struct("Data")
                .field("sequence", sequence)
                .field("stream", stream)
                .field("bytes", &bytes.len())
                .finish(),
            Self::Eof {
                after_sequence,
                stream,
            } => f
                .debug_struct("Eof")
                .field("after_sequence", after_sequence)
                .field("stream", stream)
                .finish(),
            Self::Exit {
                after_sequence,
                code,
            } => f
                .debug_struct("Exit")
                .field("after_sequence", after_sequence)
                .field("code", code)
                .finish(),
        }
    }
}
/// One receiver for one authoritative external Runtime. UI clients subscribe to
/// that Runtime's bounded screen/delivery state, never to this upstream feed.
pub struct AutomationView {
    state: Arc<State>,
}
pub(super) struct Publisher {
    state: Arc<State>,
}
struct State {
    queue: Mutex<Queue>,
    alive: AtomicBool,
}
#[derive(Default)]
struct Queue {
    events: VecDeque<AutomationViewEvent>,
    bytes: usize,
}
impl AutomationView {
    pub(super) fn new() -> (Self, Publisher) {
        let state = Arc::new(State {
            queue: Mutex::new(Queue::default()),
            alive: AtomicBool::new(true),
        });
        (
            Self {
                state: state.clone(),
            },
            Publisher { state },
        )
    }
    pub fn try_recv(&self) -> Option<AutomationViewEvent> {
        let mut queue = self.state.queue.lock().unwrap_or_else(|e| e.into_inner());
        let event = queue.events.pop_front()?;
        if let AutomationViewEvent::Data { bytes, .. } = &event {
            queue.bytes -= bytes.len();
        }
        Some(event)
    }
}
impl Drop for AutomationView {
    fn drop(&mut self) {
        self.state.alive.store(false, Ordering::Release);
    }
}
impl Publisher {
    pub fn alive(&self) -> bool {
        self.state.alive.load(Ordering::Acquire)
    }
    /// Full returns the exact event. The owner retains it and stops consuming
    /// raw output until this SAME event can be accepted, without renumbering.
    pub fn try_push(&self, event: AutomationViewEvent) -> Result<(), AutomationViewEvent> {
        let size = match &event {
            AutomationViewEvent::Data { bytes, .. } => bytes.len(),
            _ => 0,
        };
        let mut queue = self.state.queue.lock().unwrap_or_else(|e| e.into_inner());
        if !self.alive() || queue.bytes + size > AUTOMATION_CREDIT || queue.events.len() >= 64 {
            return Err(event);
        }
        queue.bytes += size;
        queue.events.push_back(event);
        Ok(())
    }
}
