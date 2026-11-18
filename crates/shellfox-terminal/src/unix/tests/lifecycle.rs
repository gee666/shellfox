use crate::unix::{Lifecycle, Ownership, Phase};
use anyhow::{ensure, Result};
use std::cell::Cell;
use std::rc::Rc;

struct Owner {
    fail_prepare: bool,
    fail_auth: bool,
    fail_poll: bool,
    root_exited: bool,
    failures: Rc<Cell<u8>>,
    closes: Rc<Cell<u8>>,
}
impl Ownership for Owner {
    fn prepare(&mut self) -> Result<()> {
        ensure!(!self.fail_prepare, "preflight");
        Ok(())
    }
    fn authenticate(&mut self) -> Result<()> {
        ensure!(!self.fail_auth, "startup");
        Ok(())
    }
    fn poll_root(&mut self) -> Result<bool> {
        ensure!(!self.fail_poll, "identity observation failed");
        Ok(self.root_exited)
    }
    fn close(&mut self) -> Result<()> {
        self.closes.set(self.closes.get() + 1);
        let remaining = self.failures.get();
        self.failures.set(remaining.saturating_sub(1));
        ensure!(remaining == 0, "cleanup failure");
        Ok(())
    }
}
fn owner() -> Owner {
    Owner {
        fail_prepare: false,
        fail_auth: false,
        fail_poll: false,
        root_exited: false,
        failures: Rc::new(Cell::new(0)),
        closes: Rc::new(Cell::new(0)),
    }
}
#[test]
fn no_allocation_before_ownership_preflight() {
    let mut o = owner();
    o.fail_prepare = true;
    let allocations = Cell::new(0);
    if let Ok(mut lifecycle) = Lifecycle::prepare(o) {
        lifecycle
            .allocate_pty(|_| {
                allocations.set(allocations.get() + 1);
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(allocations.get(), 0);
}
#[test]
fn startup_failure_retains_owner_for_cleanup_retry() {
    let mut o = owner();
    o.fail_auth = true;
    o.failures.set(1);
    let closes = o.closes.clone();
    let mut lifecycle = Lifecycle::prepare(o).unwrap();
    assert!(!lifecycle.input_allowed());
    lifecycle.allocate_pty(|_| Ok(())).unwrap();
    assert!(lifecycle.authenticate().is_err());
    assert!(lifecycle.cleanup_pending());
    assert!(lifecycle.close().is_err());
    assert!(lifecycle.cleanup_pending());
    lifecycle.close().unwrap();
    assert_eq!(lifecycle.phase(), Phase::Closed);
    assert_eq!(closes.get(), 2);
}
#[test]
fn root_exit_cleans_descendants_and_disables_input_even_on_failure() {
    let o = owner();
    o.failures.set(1);
    let mut lifecycle = Lifecycle::prepare(o).unwrap();
    lifecycle.allocate_pty(|_| Ok(())).unwrap();
    lifecycle.authenticate().unwrap();
    assert!(lifecycle.input_allowed());
    assert!(lifecycle.root_exited().is_err());
    assert!(!lifecycle.input_allowed());
    assert!(lifecycle.cleanup_pending());
    assert!(lifecycle.allocate_pty(|_| Ok(())).is_err());
    lifecycle.close().unwrap();
    assert!(lifecycle.authenticate().is_err());
}
#[test]
fn poll_uses_owned_cleanup_and_refuses_input_after_observation_failure() {
    for failure in [false, true] {
        let mut o = owner();
        o.fail_poll = failure;
        o.root_exited = !failure;
        let mut lifecycle = Lifecycle::prepare(o).unwrap();
        lifecycle.allocate_pty(|_| Ok(())).unwrap();
        lifecycle.authenticate().unwrap();
        if failure {
            assert!(lifecycle.poll().is_err());
            assert!(lifecycle.cleanup_pending());
        } else {
            assert!(lifecycle.poll().unwrap());
            assert_eq!(lifecycle.phase(), Phase::Closed);
        }
        assert!(!lifecycle.input_allowed());
        lifecycle.close().unwrap();
    }
}
#[test]
fn partial_io_failure_disables_input_without_discarding_the_owner() {
    let mut lifecycle = Lifecycle::prepare(owner()).unwrap();
    lifecycle.allocate_pty(|_| Ok(())).unwrap();
    lifecycle.authenticate().unwrap();
    lifecycle.input_failed();
    assert!(!lifecycle.input_allowed());
    assert!(lifecycle.cleanup_pending());
    lifecycle.close().unwrap();
    lifecycle.input_failed();
    assert_eq!(lifecycle.phase(), Phase::Closed);
}
#[test]
fn allocation_failure_also_retains_cleanup_resources() {
    let mut lifecycle = Lifecycle::prepare(owner()).unwrap();
    assert!(lifecycle
        .allocate_pty::<()>(|_| anyhow::bail!("PTY allocation"))
        .is_err());
    assert!(lifecycle.cleanup_pending());
    lifecycle.close().unwrap();
}
