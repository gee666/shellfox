//! Original-context FIFO. Queued failures cancel successors, never replay writes.
use super::bindings::NativeBinding;
use super::*;
use crate::client::runtime::*;
use shellfox_protocol::Validate;
#[derive(Default)]
pub(super) struct Lane {
    queue: VecDeque<Intent>,
    bytes: usize,
    running: bool,
    in_flight: bool,
}
impl Lane {
    fn take_pending(&mut self) -> VecDeque<Intent> {
        let pending = std::mem::take(&mut self.queue);
        self.bytes = self
            .bytes
            .saturating_sub(pending.iter().map(|intent| intent.charge).sum::<usize>());
        pending
    }
}
// Release the lane mutex before the existing client -> view custody checks.
// Only dequeued, definitely-unsent tails release their admitted InputBook IDs.
pub(super) fn fence(binding: &NativeBinding) -> Result<()> {
    let pending = {
        let mut lane = binding.input.lock().map_err(|_| PANIC)?;
        lane.take_pending()
    };
    cancel_queued(binding, pending);
    Ok(())
}
fn cancel_queued(binding: &NativeBinding, pending: VecDeque<Intent>) {
    for intent in pending {
        cancel_before_send(binding, &intent);
        intent.operation.cancel.cancel();
        let _ = intent.reply.send(Err(CANCELLED));
    }
}
enum RequestKind {
    Write(m::WriteTerminal),
    Resize(m::ResizeTerminal),
}
struct Intent {
    request: RequestKind,
    origin: RecoveryOrigin,
    operation: Operation,
    reply: tokio::sync::oneshot::Sender<Result<serde_json::Value>>,
    charge: usize,
}
pub(super) fn enqueue(
    mobile: Arc<Mobile>,
    engine: u64,
    input: m::Input,
    hint: bool,
) -> Result<u64> {
    input.validate().map_err(|_| INVALID)?;
    let b = super::bindings::by_engine(&mobile, engine)?;
    if b.shared.owner != std::thread::current().id() {
        return Err(crate::Status::WrongThread as i32);
    }
    let origin = b.origin()?;
    let charge = match &input {
        m::Input::Key { event } => event.text.len(),
        m::Input::Paste { text } => text.len(),
        m::Input::Bytes { .. } => input
            .decoded_bytes()
            .map_err(|_| INVALID)?
            .ok_or(INVALID)?
            .len(),
        m::Input::Mouse { .. } => 0,
    };
    if charge > shellfox_protocol::MAX_REQUEST_BYTES {
        return Err(INVALID);
    }
    accept(
        mobile,
        b,
        origin,
        charge,
        move |cursor, hints, binding| {
            let id = m::Id::new();
            let request = match input {
                m::Input::Mouse { event, geometry } => cursor.admit_mouse(
                    id,
                    event,
                    geometry.cell_width_px,
                    geometry.cell_height_px,
                    hints,
                    binding,
                ),
                other => cursor.admit_input(id, other, hints, binding),
            }
            .map_err(super::operations::cursor_error)?;
            Ok(RequestKind::Write(request))
        },
        hint,
    )
}
pub(super) fn resize(mobile: Arc<Mobile>, engine: u64, cols: u16, rows: u16) -> Result<u64> {
    m::validate_grid(cols, rows).map_err(|_| INVALID)?;
    let b = super::bindings::by_engine(&mobile, engine)?;
    if b.shared.owner != std::thread::current().id() {
        return Err(crate::Status::WrongThread as i32);
    }
    let origin = b.origin()?;
    accept(
        mobile,
        b,
        origin,
        0,
        move |cursor, _hints, binding| {
            cursor
                .resize(cols, rows, binding)
                .map(RequestKind::Resize)
                .map_err(super::operations::cursor_error)
        },
        false,
    )
}
fn accept(
    mobile: Arc<Mobile>,
    b: Arc<NativeBinding>,
    origin: RecoveryOrigin,
    charge: usize,
    prepare: impl FnOnce(&mut ManagerCursor, m::InputHints, &m::Binding) -> Result<RequestKind>,
    hint: bool,
) -> Result<u64> {
    let actual = crate::engine::lookup(b.terminal).map_err(|error| error as i32)?;
    if !Arc::ptr_eq(&actual, &b.shared) {
        return Err(STALE);
    }
    let operation = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        reserve_locked(&mut s, CompletionOrigin::NativeInternal, SMALL_RESERVE)?
    };
    let (reply, receive) = tokio::sync::oneshot::channel();
    let outcome = origin
        .witness
        .commit_if_current(|| {
            let mut state = b.state.lock().map_err(|_| PANIC)?;
            origin.check(&state).map_err(|_| STALE)?;
            let queue = b.shared.queue.lock().map_err(|_| PANIC)?;
            if queue.closed {
                return Err(STALE);
            }
            if queue.failed {
                return Err(PROTOCOL);
            }
            let mut lane = b.input.lock().map_err(|_| PANIC)?;
            if lane.queue.len() + usize::from(lane.in_flight) >= 64
                || lane
                    .bytes
                    .checked_add(charge)
                    .is_none_or(|n| n > 128 * 1024)
            {
                return Err(BUSY);
            }
            lane.queue.try_reserve(1).map_err(|_| BUSY)?;
            let settled_at = b.settled_at.lock().map_err(|_| PANIC)?.clone();
            let hints = m::InputHints {
                user_initiated: hint,
                activity_hint: hint,
                settled_at,
            };
            let request = prepare(
                state.cursor.as_mut().ok_or(BUSY)?,
                hints,
                origin.witness.binding(),
            )?;
            lane.queue.push_back(Intent {
                request,
                origin: origin.clone(),
                operation: operation.clone(),
                reply,
                charge,
            });
            lane.bytes += charge;
            let start = !lane.running;
            lane.running = true;
            Ok(start)
        })
        .map_err(super::operations::client_error)
        .and_then(|v| v);
    let start = match outcome {
        Ok(start) => start,
        Err(error) => {
            if let Ok(mut s) = mobile.state.lock() {
                unreserve(&mut s, &operation);
            }
            return Err(error);
        }
    };
    let exec = runtime()?;
    watch(exec, mobile.clone(), operation.clone(), receive);
    if start {
        exec.spawn(run(mobile, b));
    }
    Ok(operation.id)
}
async fn run(mobile: Arc<Mobile>, binding: Arc<NativeBinding>) {
    loop {
        let intent = {
            let Ok(mut lane) = binding.input.lock() else {
                return;
            };
            let Some(intent) = lane.queue.pop_front() else {
                lane.running = false;
                return;
            };
            lane.in_flight = true;
            intent
        };
        let result = CURRENT
            .scope(
                intent.operation.clone(),
                perform(&mobile, &binding, &intent),
            )
            .await;
        let success = result
            .as_ref()
            .is_ok_and(|v| v.get("ok").and_then(serde_json::Value::as_bool) == Some(true));
        let pending = if let Ok(mut lane) = binding.input.lock() {
            lane.in_flight = false;
            lane.bytes = lane.bytes.saturating_sub(intent.charge);
            if !success {
                let pending = lane.take_pending();
                lane.running = false;
                pending
            } else {
                VecDeque::new()
            }
        } else {
            VecDeque::new()
        };
        cancel_queued(&binding, pending);
        let _ = intent.reply.send(result);
        if !success {
            return;
        }
    }
}
async fn perform(
    mobile: &Mobile,
    binding: &NativeBinding,
    intent: &Intent,
) -> Result<serde_json::Value> {
    if intent.operation.cancel.is_cancelled() {
        cancel_before_send(binding, intent);
        return Err(CANCELLED);
    }
    let live = intent
        .origin
        .witness
        .commit_if_current(|| {
            let state = binding.state.lock().map_err(|_| PANIC)?;
            intent.origin.check(&state).map_err(|_| STALE)?;
            let actual = crate::engine::lookup(binding.terminal).map_err(|error| error as i32)?;
            if !Arc::ptr_eq(&actual, &binding.shared) {
                return Err(STALE);
            }
            let queue = binding.shared.queue.lock().map_err(|_| PANIC)?;
            if queue.closed {
                return Err(STALE);
            }
            if queue.failed {
                return Err(PROTOCOL);
            }
            Ok(())
        })
        .map_err(super::operations::client_error)
        .and_then(|value| value);
    if let Err(error) = live {
        cancel_before_send(binding, intent);
        return Err(error);
    }
    let client = mobile.manager()?;
    match &intent.request {
        RequestKind::Write(request) => {
            let result = client
                .execute_with_witness(&intent.origin.witness, request)
                .await;
            match result {
                Ok(m::Reply::Success(receipt)) => {
                    // The actual validated receipt is historical. A retired view
                    // cannot revive authority, but it cannot turn this into zero.
                    let _ = intent.origin.witness.commit_if_current(|| {
                        if let Ok(mut state) = binding.state.lock() {
                            if intent.origin.check(&state).is_ok() {
                                if let Some(cursor) = state.cursor.as_mut() {
                                    let _ = cursor
                                        .accepted(receipt.clone(), intent.origin.witness.binding());
                                }
                            }
                        }
                    });
                    serde_json::to_value(m::Reply::Success(receipt)).map_err(|_| PROTOCOL)
                }
                Ok(m::Reply::Failure(error)) => {
                    mark_unknown(binding, intent, request.input_id);
                    serde_json::to_value(m::Reply::<m::AcceptedInput>::Failure(error))
                        .map_err(|_| PROTOCOL)
                }
                Err(error) => {
                    mark_unknown(binding, intent, request.input_id);
                    Err(super::operations::client_error(error))
                }
            }
        }
        RequestKind::Resize(request) => serde_json::to_value(
            client
                .execute_with_witness(&intent.origin.witness, request)
                .await
                .map_err(super::operations::client_error)?,
        )
        .map_err(|_| PROTOCOL),
    }
}
fn cancel_before_send(binding: &NativeBinding, intent: &Intent) {
    let RequestKind::Write(request) = &intent.request else {
        return;
    };
    let _ = intent.origin.witness.commit_if_current(|| {
        if let Ok(mut state) = binding.state.lock() {
            if intent.origin.check(&state).is_ok() {
                if let Some(cursor) = state.cursor.as_mut() {
                    let _ = cursor.cancel_before_send(request.input_id);
                }
            }
        }
    });
}
fn mark_unknown(binding: &NativeBinding, intent: &Intent, id: m::Id) {
    let _ = intent.origin.witness.commit_if_current(|| {
        if let Ok(mut state) = binding.state.lock() {
            if intent.origin.check(&state).is_ok() {
                if let Some(cursor) = state.cursor.as_mut() {
                    let _ = cursor.input_unknown(id);
                }
            }
        }
    });
}
