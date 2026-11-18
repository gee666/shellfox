use std::{cell::Cell, rc::Rc};
/// A lease tracks the real future, not a spinner that cancellation may hide.
#[derive(Default)]
pub(super) struct Operations {
    active: Rc<Cell<usize>>,
    frozen: bool,
}
pub(super) struct Lease(Rc<Cell<usize>>);
impl Operations {
    pub fn begin(&self) -> Lease {
        self.active.set(self.active.get() + 1);
        Lease(self.active.clone())
    }
    pub fn pending(&self) -> bool {
        self.active.get() != 0
    }
    pub fn frozen(&self) -> bool {
        self.frozen
    }
    pub fn try_freeze(&mut self, other_pending: bool) -> bool {
        if self.frozen {
            return true;
        }
        if other_pending || self.pending() {
            return false;
        }
        self.frozen = true;
        true
    }
    pub fn resume(&mut self) {
        self.frozen = false;
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
#[cfg(test)]
mod tests {
    use super::Operations;
    #[test]
    fn idle_is_not_pending_and_freeze_does_not_invent_work() {
        let mut operations = Operations::default();
        assert!(!operations.pending());
        assert!(operations.try_freeze(false));
        assert!(operations.frozen());
        assert!(!operations.pending());
        operations.resume();
        assert!(!operations.frozen());
    }
    #[test]
    fn leases_remain_pending_until_real_completion_even_after_ui_cancellation() {
        let mut operations = Operations::default();
        let first = operations.begin();
        let second = operations.begin();
        assert!(!operations.try_freeze(false));
        drop(first);
        assert!(operations.pending());
        drop(second);
        assert!(!operations.pending());
        assert!(operations.try_freeze(false));
    }
    #[test]
    fn a_queued_debounce_prevents_freeze_without_latching_it() {
        let mut operations = Operations::default();
        assert!(!operations.try_freeze(true));
        assert!(!operations.frozen());
        assert!(operations.try_freeze(false));
    }
}
