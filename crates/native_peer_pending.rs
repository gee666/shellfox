//! Admitted IDs must be retired before their never-sent queue entries disappear.
use shellfox_mobile::PendingManagerInput;
use std::collections::VecDeque;
#[cfg(test)]
#[path = "native_peer_pending_tests.rs"]
mod tests;
#[derive(Default)]
pub struct PendingInputs(VecDeque<PendingManagerInput>);
impl PendingInputs {
    pub fn pending_count(&self) -> usize {
        self.0.len()
    }
    pub fn push(&mut self, input: PendingManagerInput) {
        self.0.push_back(input);
    }
    pub fn pop(&mut self) -> Option<PendingManagerInput> {
        self.0.pop_front()
    }
    pub fn cancel_unsent(&mut self) -> bool {
        let mut confirmed = true;
        while let Some(input) = self.0.pop_front() {
            confirmed &= input.cancel_before_send().is_ok();
        }
        confirmed
    }
}
impl Drop for PendingInputs {
    fn drop(&mut self) {
        self.cancel_unsent();
    }
}
