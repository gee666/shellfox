use super::{Outbox, Pending, intent::Intent};
#[cfg(test)]
use shellfox_runtime::Request;
use shellfox_runtime::SubmitError;
use std::collections::HashMap;
#[cfg(test)]
mod tests;

/// Bounded intents transferred when a view disappears. Presses already queued
/// to the worker stay ahead of this queue, including their reserved releases.
pub(super) struct Retirement {
    outbox: Outbox,
    next: u64,
    pub needs_detach: bool,
}
impl Retirement {
    pub fn new(outbox: Outbox, next: u64, needs_detach: bool) -> Self {
        Self {
            outbox,
            next,
            needs_detach,
        }
    }
    #[cfg(test)]
    pub fn next(&mut self) -> Option<Request> {
        self.next_intent().map(|intent| intent.request)
    }
    pub fn next_intent(&mut self) -> Option<Intent> {
        let mut slot = None;
        let mut pending = HashMap::<u64, Pending>::new();
        let _ = self
            .outbox
            .flush_with_input(&mut self.next, &mut pending, |request, input| {
                if slot.is_none() {
                    slot = Some(Intent {
                        request,
                        input: input.cloned(),
                    });
                    Ok(())
                } else {
                    Err(SubmitError::Full(request))
                }
            });
        slot
    }
}
