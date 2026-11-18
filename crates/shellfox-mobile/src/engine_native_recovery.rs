//! Owner-thread decode/install only. No native call or old-engine drop under the binding fence.
use super::*;
use crate::client::manager::{PreparedNative0Install, RecoveryWork};
use crate::queue::Payload;
pub(crate) fn prepare_native0_pending(owned: &mut Owned) -> Result<PreparedNative0Install> {
    let (ticket, work) = {
        let mut queue = owned.shared.queue.lock().map_err(|_| Status::Panic)?;
        if !matches!(queue.chunks.front(), Some((_, Payload::Native0(_)))) {
            return Err(Status::Busy);
        }
        let (ticket, payload) = queue.chunks.pop_front().ok_or(Status::Busy)?;
        queue.bytes -= payload.len();
        let Payload::Native0(work) = payload else {
            return Err(Status::Invalid);
        };
        (ticket, *work)
    };
    prepare_work(owned, ticket, work)
}
pub(super) fn prepare_work(
    owned: &mut Owned,
    ticket: u64,
    work: RecoveryWork,
) -> Result<PreparedNative0Install> {
    if work.origin.cancel.is_cancelled() || !Arc::ptr_eq(&owned.shared, &work.shared) {
        return Err(Status::Stale);
    }
    // Decode and native validation are OUTSIDE client/state/queue locks. A bad
    // artifact never resets or poisons the already-live terminal.
    let result = crate::client::decode_native0(
        work.cursor.recovery(),
        work.origin.witness.selection(),
        &work.receiver,
        work.limits,
    );
    match result {
        Ok(terminal) => Ok(PreparedNative0Install {
            terminal,
            work,
            ticket,
        }),
        Err(error) => {
            let _ = work.origin.witness.commit_if_current(|| {
                if let Ok(mut state) = work.state.lock() {
                    if work.origin.check(&state).is_ok() {
                        state.failure = Some(error);
                    }
                }
            });
            Err(error)
        }
    }
}
pub(crate) fn commit_native0(owned: &mut Owned, prepared: PreparedNative0Install) -> Result<()> {
    let PreparedNative0Install {
        terminal,
        work,
        ticket,
    } = prepared;
    let id = work.cursor.recovery().recovery_id();
    let mut candidate = Some(terminal);
    let mut cursor = Some(work.cursor);
    let mut retired_terminal = None;
    let mut retired_cursor = None;
    // Explicit lock order: ACTUAL client binding/cycle -> SDK state -> queue.
    // All allocations/native decode were completed above. After validation,
    // only infallible local swaps and the real consumed ticket are committed.
    let result = work
        .origin
        .witness
        .commit_if_current(|| {
            let mut state = work.state.lock().map_err(|_| Status::Panic)?;
            let mut queue = owned.shared.queue.lock().map_err(|_| Status::Panic)?;
            if work.origin.check(&state).is_err()
                || !Arc::ptr_eq(&owned.shared, &work.shared)
                || queue.failed
                || queue.closed
                || queue.consumed.checked_add(1) != Some(ticket)
                || queue.accepted < ticket
            {
                return Err(Status::Stale);
            }
            if candidate.is_none() || cursor.is_none() {
                return Err(Status::Invalid);
            }
            // No fallible step after taking the native candidate. A rejected
            // candidate remains outside this closure for post-fence destruction.
            let Some(terminal) = candidate.take() else {
                return Err(Status::Invalid);
            };
            retired_terminal = Some(std::mem::replace(&mut owned.terminal, terminal));
            retired_cursor = std::mem::replace(&mut state.cursor, cursor.take());
            state.installed = Some((ticket, id));
            state.origin = Some(work.origin.clone());
            state.failure = None;
            state.pending_ack = None;
            state.data.clear();
            queue.consumed = ticket;
            Ok(())
        })
        .map_err(|_| Status::Stale)
        .and_then(|result| result);
    // A rejected candidate and a retired native engine may invoke native frees.
    // They are dropped AFTER the actual binding mutex is released.
    drop(retired_terminal);
    drop(retired_cursor);
    drop(candidate);
    drop(cursor);
    result
}
