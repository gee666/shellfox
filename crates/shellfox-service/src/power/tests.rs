use super::{PowerBackend, PowerError, PowerLease, PowerManager, PowerStatus};
use std::sync::{Arc, Mutex};

mod worker_cases;

#[derive(Default)]
struct State {
    acquires: u32,
    releases: u32,
    drops: u32,
    fail_acquire: bool,
    fail_release: bool,
    partial: bool,
}
struct Backend(Arc<Mutex<State>>);
struct Lease(Arc<Mutex<State>>, bool);
impl PowerBackend for Backend {
    type Lease = Lease;
    fn acquire(&mut self) -> Result<Lease, PowerError> {
        let mut s = self.0.lock().unwrap();
        s.acquires += 1;
        if s.fail_acquire {
            return Err(PowerError("acquire failed".into()));
        }
        Ok(Lease(self.0.clone(), true))
    }
}
impl PowerLease for Lease {
    fn partial_reason(&self) -> Option<&str> {
        if self.0.lock().unwrap().partial {
            Some("lid denied")
        } else {
            None
        }
    }
    fn release(&mut self) -> Result<(), PowerError> {
        let mut s = self.0.lock().unwrap();
        s.releases += 1;
        if s.fail_release {
            return Err(PowerError("release failed".into()));
        }
        self.1 = false;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.1 {
            self.0.lock().unwrap().drops += 1;
        }
    }
}

#[test]
fn system_manager_starts_off_without_acquiring() {
    assert_eq!(PowerManager::new().status(), PowerStatus::Inactive);
}

#[test]
fn toggles_are_idempotent_and_release_the_owned_lease() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut manager = PowerManager::with_backend(Backend(state.clone()));
    assert_eq!(manager.status(), PowerStatus::Inactive);
    assert_eq!(manager.set(false).unwrap(), PowerStatus::Inactive);
    assert_eq!(manager.set(true).unwrap(), PowerStatus::Active);
    assert_eq!(manager.set(true).unwrap(), PowerStatus::Active);
    assert_eq!(manager.set(false).unwrap(), PowerStatus::Inactive);
    assert_eq!(manager.set(false).unwrap(), PowerStatus::Inactive);
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.releases, state.drops), (1, 1, 0));
}

#[test]
fn failures_are_visible_and_failed_release_is_retained_for_retry() {
    let state = Arc::new(Mutex::new(State {
        fail_acquire: true,
        ..State::default()
    }));
    let mut manager = PowerManager::with_backend(Backend(state.clone()));
    assert!(manager.set(true).is_err());
    assert_eq!(manager.status(), PowerStatus::Inactive);
    state.lock().unwrap().fail_acquire = false;
    manager.set(true).unwrap();
    state.lock().unwrap().fail_release = true;
    assert!(manager.set(false).is_err());
    assert_eq!(manager.status(), PowerStatus::ReleaseFailed);
    assert!(manager.set(true).is_err());
    state.lock().unwrap().fail_release = false;
    assert_eq!(manager.set(false).unwrap(), PowerStatus::Inactive);
    let state = state.lock().unwrap();
    assert_eq!((state.acquires, state.releases, state.drops), (2, 2, 0));
}

#[test]
fn dropping_each_manager_only_drops_its_own_lease() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut a = PowerManager::with_backend(Backend(state.clone()));
    let mut b = PowerManager::with_backend(Backend(state.clone()));
    a.set(true).unwrap();
    b.set(true).unwrap();
    drop(a);
    assert_eq!(state.lock().unwrap().drops, 1);
    assert_eq!(b.status(), PowerStatus::Active);
    drop(b);
    assert_eq!(state.lock().unwrap().drops, 2);
}

#[test]
fn partial_capability_is_reported_and_can_be_released() {
    let state = Arc::new(Mutex::new(State {
        partial: true,
        ..State::default()
    }));
    let mut manager = PowerManager::with_backend(Backend(state));
    assert_eq!(manager.set(true).unwrap(), PowerStatus::Partial);
    assert_eq!(manager.partial_reason(), Some("lid denied"));
    assert_eq!(manager.set(false).unwrap(), PowerStatus::Inactive);
    assert_eq!(manager.partial_reason(), None);
}

#[cfg(windows)]
#[test]
fn windows_real_request_is_brief_and_released() {
    let mut manager = PowerManager::new();
    assert_eq!(manager.set(true).unwrap(), PowerStatus::Active);
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(manager.set(false).unwrap(), PowerStatus::Inactive);
}
