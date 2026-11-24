use super::{operations, runtime::*, terminal::Binding};
use shellfox_protocol::*;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::oneshot;

pub(super) const MAX_INTENTS: usize = 64;
/// Input waiting in one lane; typing past it gets BUSY.
pub(super) const MAX_BYTES: usize = 1024 * 1024;
struct Intent {
    operation: Operation,
    command: RemoteCommand,
    bytes: usize,
    /// The lane's resize count when this was admitted.
    resizes: u64,
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
    reattach: bool,
    /// JS was asked to reattach and no attach has finished since.
    asked: bool,
    /// Counts resizes; a delayed retry of a refused one yields to any newer.
    resizes: u64,
    resize_retries: u8,
    /// A lost resize reattached once. Until a resize succeeds, another loss
    /// only drops it: a host that always refuses must not loop reattaches.
    resize_reattached: bool,
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
            asked: false,
            resizes: 0,
            resize_retries: 0,
            resize_reattached: false,
        }
    }
}
impl MutationLane {
    pub fn available(&self) -> bool {
        !self.failed
    }
    /// Whether JS should reattach this terminal; asks once.
    pub fn take_reattach(&mut self) -> bool {
        let wanted = std::mem::take(&mut self.reattach);
        self.asked |= wanted;
        wanted
    }
    /// An attach finished. If it failed, the next refused input asks again.
    pub fn attach_finished(&mut self) {
        self.asked = false;
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
        self.asked = false;
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
    admit(mobile, command, origin, None)
}
/// A lane fenced by a lost write whose reattach request never led anywhere
/// (the attach failed, or another one was in flight): ask again.
fn needs_reattach(b: &Binding, command: &RemoteCommand, connection: u64) -> bool {
    let Some(cursor) = b.cursor.as_ref() else {
        return false;
    };
    let owned = match command {
        RemoteCommand::WriteTerminal(v) => cursor
            .write(v.data.clone(), v.user_initiated, connection)
            .is_ok(),
        RemoteCommand::ResizeTerminal(v) => cursor.resize(v.cols, v.rows, connection).is_ok(),
        _ => false,
    };
    owned
        && b.lane.failed
        && !b.lane.reattach
        && !b.lane.asked
        && b.attaching.is_none()
        && !b.reset_required
}
/// `retry_of`: a delayed resize retry, which runs only if no resize came since.
fn admit(
    mobile: Arc<Mobile>,
    command: RemoteCommand,
    origin: CompletionOrigin,
    retry_of: Option<u64>,
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
        let b = s.bindings.get_mut(&tab).ok_or(STALE)?;
        if let Err(error) = validate(b, &command, connection.connection_id) {
            let ask = error == STALE && needs_reattach(b, &command, connection.connection_id);
            if ask {
                b.lane.reattach = true;
                drop(s);
                // The completion carries the reattach request to JS.
                let _ = launch_small(mobile, CompletionOrigin::NativeInternal, |_| async {
                    Err(STALE)
                });
            }
            return Err(error);
        }
        if let RemoteCommand::ResizeTerminal(_) = &command {
            if retry_of.is_some_and(|seen| seen != b.lane.resizes) {
                return Err(STALE);
            }
            if retry_of.is_none() {
                b.lane.resize_retries = 0;
            }
            b.lane.resizes = b.lane.resizes.wrapping_add(1);
        }
        let b = &*b;
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
        if b.lane.bytes + bytes > MAX_BYTES {
            return Err(BUSY);
        }
        let b = s.bindings.get_mut(&tab).ok_or(STALE)?;
        let command = match join(&mut b.lane, command) {
            Ok(operation) => return Ok(operation),
            Err(command) => command,
        };
        if b.lane.queue.len() + usize::from(b.lane.active.is_some()) >= MAX_INTENTS {
            return Err(BUSY);
        }
        let operation = reserve_locked(&mut s, origin, SMALL_RESERVE)?;
        let b = s.bindings.get_mut(&tab).ok_or(STALE)?;
        let (reply, receive) = oneshot::channel();
        b.lane.bytes += bytes;
        let resizes = b.lane.resizes;
        b.lane.queue.push_back(Intent {
            operation: operation.clone(),
            command,
            bytes,
            resizes,
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
/// Appends a write to the lane's last waiting write, so a burst of typing
/// goes out in a few requests instead of filling the lane. The joined input
/// completes with that write's operation, whose id is returned.
fn join(
    lane: &mut MutationLane,
    command: RemoteCommand,
) -> std::result::Result<u64, RemoteCommand> {
    let RemoteCommand::WriteTerminal(write) = &command else {
        return Err(command);
    };
    let Some(tail) = lane.queue.back_mut() else {
        return Err(command);
    };
    match &mut tail.command {
        RemoteCommand::WriteTerminal(waiting)
            if !tail.operation.cancel.is_cancelled()
                && waiting.tab_id == write.tab_id
                && waiting.generation == write.generation
                && waiting.control_epoch == write.control_epoch
                && waiting.user_initiated == write.user_initiated
                && waiting.settled_at == write.settled_at
                && waiting.data.len() + write.data.len() <= crate::MAX_INPUT =>
        {
            waiting.data.push_str(&write.data);
            tail.bytes += write.data.len();
            lane.bytes += write.data.len();
            Ok(tail.operation.id)
        }
        _ => Err(command),
    }
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
        let resize = match &intent.command {
            RemoteCommand::ResizeTerminal(v) => Some(v.clone()),
            _ => None,
        };
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
        let mut outcome = if operation.cancel.is_cancelled() {
            Outcome::Lost
        } else {
            outcome(&result)
        };
        let mut retry = None;
        let current = {
            let Ok(mut s) = mobile.state.lock() else {
                return;
            };
            if let Some(b) = s.bindings.get_mut(&tab) {
                if b.lane.serial == serial {
                    b.lane.active = None;
                    b.lane.bytes -= intent.bytes;
                    if let Some(size) = &resize {
                        match outcome {
                            Outcome::Done => {
                                b.lane.resize_reattached = false;
                                b.lane.resize_retries = 0;
                            }
                            Outcome::Lost if !b.lane.resize_reattached => {
                                b.lane.resize_reattached = true;
                            }
                            _ => outcome = Outcome::Rejected,
                        }
                        // A size the host refused never applied. Ask again
                        // shortly, as a terminal still starting refuses it.
                        if outcome == Outcome::Rejected && b.lane.resize_retries < RESIZE_RETRIES {
                            b.lane.resize_retries += 1;
                            retry = Some((size.clone(), intent.resizes));
                        }
                    }
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
        if let (true, Some((size, seen)), Ok(executor)) = (current, retry, runtime()) {
            let owner = mobile.clone();
            executor.spawn(async move {
                tokio::time::sleep(RESIZE_RETRY_DELAY).await;
                let command = RemoteCommand::ResizeTerminal(size);
                let _ = admit(owner, command, CompletionOrigin::NativeInternal, Some(seen));
            });
        }
        if !current || outcome == Outcome::Lost {
            return;
        }
    }
}

const RESIZE_RETRIES: u8 = 3;
const RESIZE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
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
