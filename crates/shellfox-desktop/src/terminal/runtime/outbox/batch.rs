//! Memory accounting and key batching for the outbox.
use super::Queued;
use crate::terminal::runtime::Pending;
use shellfox_ghostty_vt::key::KeyEvent;
use shellfox_runtime::{Command, MAX_KEY_BATCH};
use shellfox_service::native::NativeInputContext;

/// Queued input above this many bytes pauses typing until half has drained.
pub(super) const MAX_PENDING_BYTES: usize = 1024 * 1024;
/// Charged per queued event, so empty events such as releases count too.
const EVENT_COST: usize = 16;
/// Keeps a batch's encoded bytes well inside the host's 64 KiB write.
const BATCH_TEXT_BYTES: usize = 2048;
pub(crate) const PAUSED: &str = "Input paused, the terminal isn't keeping up";

pub(super) fn cost(command: &Command) -> usize {
    match command {
        Command::Paste { text, .. } => EVENT_COST + text.len(),
        Command::RawInput { bytes, .. } => EVENT_COST + bytes.len(),
        Command::Key { event, .. } => EVENT_COST + event.text.len(),
        Command::Keys { events, .. } => events
            .iter()
            .map(|event| EVENT_COST + event.text.len())
            .sum(),
        _ => EVENT_COST,
    }
}

/// The same input context apart from the hint, which a batch ORs together.
fn same_context(a: &Option<NativeInputContext>, b: &Option<NativeInputContext>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.captured_settlement == b.captured_settlement && a.user_initiated == b.user_initiated
        }
        (None, None) => true,
        _ => false,
    }
}

/// Appends a key to a queued key or batch that has not been sent yet, so a
/// backlog of typing goes to the host in a few requests. Returns the key back
/// when it cannot join the tail.
pub(super) fn join(
    tail: Option<&mut Queued>,
    command: Command,
    pending: &Pending,
    input: &Option<NativeInputContext>,
) -> Result<(), Command> {
    let Some(tail) = tail else {
        return Err(command);
    };
    let Command::Key { lease, event } = command else {
        return Err(command);
    };
    let fits = |events: &[KeyEvent]| {
        events.len() < MAX_KEY_BATCH
            && events.iter().map(|key| key.text.len()).sum::<usize>() + event.text.len()
                <= BATCH_TEXT_BYTES
    };
    let joinable = matches!(pending, Pending::Applied)
        && matches!(tail.pending, Pending::Applied)
        && same_context(&tail.input, input)
        && match &tail.command {
            Command::Key {
                lease: tail_lease,
                event: first,
            } => *tail_lease == lease && fits(std::slice::from_ref(first)),
            Command::Keys {
                lease: tail_lease,
                events,
            } => *tail_lease == lease && fits(events),
            _ => false,
        };
    if !joinable {
        return Err(Command::Key { lease, event });
    }
    if let (Some(tail), Some(input)) = (tail.input.as_mut(), input) {
        tail.hint |= input.hint;
    }
    tail.cost += EVENT_COST + event.text.len();
    if let Command::Key { event: first, .. } = &tail.command {
        tail.command = Command::Keys {
            lease,
            events: vec![first.clone()],
        };
    }
    if let Command::Keys { events, .. } = &mut tail.command {
        events.push(event);
    }
    Ok(())
}
