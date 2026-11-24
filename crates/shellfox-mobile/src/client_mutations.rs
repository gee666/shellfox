use super::{operations, runtime::*, terminal::Binding};
use shellfox_protocol::*;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::oneshot;

pub(super) const MAX_INTENTS: usize = 64;
pub(super) const MAX_BYTES: usize = 128 * 1024;
struct Intent {
    operation: Operation,
    command: RemoteCommand,
    bytes: usize,
    reply: oneshot::Sender<Result<serde_json::Value>>,
}
pub(super) struct MutationLane {
    serial: u64,
    failed: bool,
    running: bool,
    active: Option<Operation>,
    queue: VecDeque<Intent>,
    bytes: usize,
    /// Input or a resize failed in a way only a fresh attachment repairs.
    pub reattach: bool,
}
impl Default for MutationLane {
    fn default() -> Self {
        Self {
            serial: 0,
            failed: true,
            running: false,
            active: None,
            queue: VecDeque::new(),
            bytes: 0,
            reattach: false,
        }
    }
}
impl MutationLane {
    pub fn available(&self) -> bool {
        !self.failed
    }
    pub fn fence(&mut self) {
        self.serial = self.serial.saturating_add(1);
        self.failed = true;
        self.running = false;
        if let Some(active) = self.active.take() {
            active.cancel.cancel();
        }
        for intent in self.queue.drain(..) {
            intent.operation.cancel.cancel();
        }
        self.bytes = 0;
    }
    /// A new attachment opens a fresh lane. Input that failed before it is
    /// never replayed: its intent was cancelled with the old lane.
    pub fn open(&mut self) -> Result<()> {
        self.fence();
        if self.serial == u64::MAX {
            return Err(BUSY);
        }
        self.failed = false;
        self.reattach = false;
        Ok(())
    }
}
impl Drop for MutationLane {
    fn drop(&mut self) {
        self.fence();
    }
}

pub(super) fn validate(binding: &Binding, command: &RemoteCommand, connection: u64) -> Result<()> {
    if binding.attaching.is_some() || binding.reset_required {
        return Err(BUSY);
    }
    if binding.lane.failed {
        return Err(STALE);
    }
    if !super::terminal::ready(binding)? {
        return Err(BUSY);
    }
    let cursor = binding.cursor.as_ref().ok_or(STALE)?;
    let valid = match command {
        RemoteCommand::WriteTerminal(v) => {
            let expected = cursor
                .write(v.data.clone(), v.user_initiated, connection)
                .map_err(|_| STALE)?;
            expected.generation == v.generation && expected.control_epoch == v.control_epoch
        }
        RemoteCommand::ResizeTerminal(v) => {
            let expected = cursor
                .resize(v.cols, v.rows, connection)
                .map_err(|_| STALE)?;
            expected.generation == v.generation && expected.control_epoch == v.control_epoch
        }
        _ => return Err(INVALID),
    };
    if valid {
        Ok(())
    } else {
        Err(STALE)
    }
}
pub(super) fn enqueue(
    mobile: Arc<Mobile>,
    command: RemoteCommand,
    origin: CompletionOrigin,
) -> Result<u64> {
    let executor = runtime()?;
    if matches!(command, RemoteCommand::WriteTerminal(_))
        && origin != CompletionOrigin::NativeInternal
    {
        return Err(INVALID);
    }
    command.validate().map_err(|_| INVALID)?;
    let tab = match &command {
        RemoteCommand::WriteTerminal(v) => v.tab_id,
        RemoteCommand::ResizeTerminal(v) => v.tab_id,
        _ => return Err(INVALID),
    };
    let connection = mobile.client.state();
    if connection.status != shellfox_client::ConnectionStatus::Connected {
        return Err(STALE);
    }
    let (operation, receive, start, serial) = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        let b = s.bindings.get(&tab).ok_or(STALE)?;
        validate(b, &command, connection.connection_id)?;
        // Freeze native settlement confirmation at acceptance, not when a
        // Tokio worker eventually dequeues the input after later host events.
        let command = if origin == CompletionOrigin::NativeInternal {
            match command {
                RemoteCommand::WriteTerminal(v) => RemoteCommand::WriteTerminal(
                    b.cursor
                        .as_ref()
                        .ok_or(STALE)?
                        .write(v.data, v.user_initiated, connection.connection_id)
                        .map_err(|_| STALE)?,
                ),
                value => value,
            }
        } else {
            command
        };
        let bytes = match &command {
            RemoteCommand::WriteTerminal(v) => {
                v.data.len()
                    + v.settled_at
                        .as_ref()
                        .and_then(Option::as_ref)
                        .map_or(0, String::len)
            }
            RemoteCommand::ResizeTerminal(_) => std::mem::size_of::<ResizeTerminal>(),
            _ => return Err(INVALID),
        };
        if b.lane.queue.len() + usize::from(b.lane.active.is_some()) >= MAX_INTENTS
            || b.lane.bytes + bytes > MAX_BYTES
        {
            return Err(BUSY);
        }
        let operation = reserve_locked(&mut s, origin, SMALL_RESERVE)?;
        let b = s.bindings.get_mut(&tab).ok_or(STALE)?;
        let (reply, receive) = oneshot::channel();
        b.lane.bytes += bytes;
        b.lane.queue.push_back(Intent {
            operation: operation.clone(),
            command,
            bytes,
            reply,
        });
        let start = !b.lane.running;
        b.lane.running = true;
        (operation, receive, start, b.lane.serial)
    };
    let id = operation.id;
    watch(executor, mobile.clone(), operation, receive);
    if start {
        executor.spawn(drain(mobile, tab, serial));
    }
    Ok(id)
}
async fn drain(mobile: Arc<Mobile>, tab: TabId, serial: u64) {
    loop {
        let next = {
            let Ok(mut s) = mobile.state.lock() else {
                return;
            };
            let Some(b) = s.bindings.get_mut(&tab) else {
                return;
            };
            if b.lane.serial != serial || b.lane.failed {
                return;
            }
            match b.lane.queue.pop_front() {
                Some(intent) => {
                    b.lane.active = Some(intent.operation.clone());
                    Some(intent)
                }
                None => {
                    b.lane.running = false;
                    None
                }
            }
        };
        let Some(intent) = next else {
            return;
        };
        let owner = mobile.clone();
        let operation = intent.operation.clone();
        let resize = matches!(intent.command, RemoteCommand::ResizeTerminal(_));
        let result = if operation.cancel.is_cancelled() {
            Err(CANCELLED)
        } else {
            let task = runtime().map(|r| {
                r.spawn(CURRENT.scope(operation.clone(), async move {
                    let _mutation = mutation_slot(&owner).await?;
                    operations::command(owner.clone(), intent.command).await
                }))
            });
            match task {
                Ok(task) => task.await.unwrap_or(Err(PANIC)),
                Err(error) => Err(error),
            }
        };
        let outcome = if operation.cancel.is_cancelled() {
            Outcome::Lost
        } else {
            match outcome(&result) {
                // A refused size is never retried in place: the fresh
                // attachment's fit sends the current one again.
                Outcome::Rejected if resize => Outcome::Lost,
                outcome => outcome,
            }
        };
        let current = {
            let Ok(mut s) = mobile.state.lock() else {
                return;
            };
            if let Some(b) = s.bindings.get_mut(&tab) {
                if b.lane.serial == serial {
                    b.lane.active = None;
                    b.lane.bytes -= intent.bytes;
                    if outcome == Outcome::Lost {
                        // The host no longer takes input from this attachment, or
                        // the write may or may not have run. Later input must wait
                        // for a fresh attachment, as 0.9.15 reattached.
                        b.lane.fence();
                        b.lane.reattach = true;
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        let _ = intent.reply.send(if current { result } else { Err(STALE) });
        if !current || outcome == Outcome::Lost {
            return;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done,
    /// The host refused this one input (validation, rate, not ready yet): it
    /// never ran, and later input still goes, as with 0.9.15.
    Rejected,
    /// Control or the terminal is gone, or the write may or may not have run.
    Lost,
}
fn outcome(result: &Result<serde_json::Value>) -> Outcome {
    let Ok(value) = result else {
        return Outcome::Lost;
    };
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Outcome::Done;
    }
    match value
        .pointer("/error/code")
        .and_then(serde_json::Value::as_str)
    {
        Some("TARGET_LOST" | "TERMINAL_CONTROL_REQUIRED") | None => Outcome::Lost,
        Some(_) => Outcome::Rejected,
    }
}
