use super::*;
#[path = "app_host_lifecycle_tests.rs"]
mod lifecycle_tests;
use std::{
    future::Future,
    pin::pin,
    task::{Context as TaskContext, Poll, Waker},
};

#[derive(Default)]
struct FakeState {
    next: usize,
    calls: Vec<String>,
    fail_detach: bool,
    fail_stop: bool,
    wait_stop: bool,
    stop_admitted: bool,
    fail_fork: bool,
    owned_terminals: usize,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);
impl AppHostBackend for Fake {
    type Client = usize;
    fn owned_terminal_count(&self) -> HostFuture<'_, usize> {
        Box::pin(async {
            let mut state = self.0.lock().unwrap();
            state.calls.push("status".into());
            Ok(state.owned_terminals)
        })
    }
    fn fork_window(&self) -> HostFuture<'_, usize> {
        Box::pin(async {
            let mut state = self.0.lock().unwrap();
            ensure!(!state.fail_fork, "controlled fork failure");
            state.next += 1;
            let client = state.next;
            state.calls.push(format!("fork:{client}"));
            Ok(client)
        })
    }
    fn detach_window<'a>(&'a self, client: &'a usize) -> HostFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.0.lock().unwrap();
            state.calls.push(format!("detach:{client}"));
            ensure!(!state.fail_detach, "controlled release failure");
            Ok(())
        })
    }
    fn stop(&self) -> HostFuture<'_, ()> {
        Box::pin(async {
            let wait = {
                let mut state = self.0.lock().unwrap();
                if !state.stop_admitted {
                    state.calls.push("stop".into());
                    ensure!(!state.fail_stop, "controlled cleanup failure");
                    state.stop_admitted = true;
                }
                state.wait_stop
            };
            if wait {
                std::future::pending::<()>().await
            }
            Ok(())
        })
    }
}
fn approved(decision: CloseDecision) -> CloseTicket {
    match decision {
        CloseDecision::Ready(ticket) => ticket,
        CloseDecision::Confirm(confirmation) => confirmation.approve(),
        CloseDecision::Blocked(guards) => panic!("unexpected guards {guards:?}"),
    }
}
fn poll_pending(future: std::pin::Pin<&mut impl Future>) {
    assert!(matches!(
        future.poll(&mut TaskContext::from_waker(Waker::noop())),
        Poll::Pending
    ));
}
#[tokio::test(flavor = "current_thread")]
async fn two_windows_share_backend_but_fork_distinct_clients_and_close_only_own_intents() {
    let fake = Fake::default();
    let application;
    {
        let host = AppHost::with_backend(fake.clone());
        let a = host.open_window().await.unwrap();
        let b = host.open_window().await.unwrap();
        assert_ne!(a.client, b.client);
        let ticket = match host.request_close(CloseTarget::Window(a.id), 5).unwrap() {
            CloseDecision::Ready(ticket) => ticket,
            _ => panic!("secondary close must not confirm host shutdown"),
        };
        let closed = host.finish_close(ticket).await.unwrap();
        assert_eq!(
            closed,
            ClosedWindows {
                windows: vec![a.id],
                app_stopped: false
            }
        );
        assert_eq!(host.window_count().unwrap(), 1);
        assert!(!host.is_window_available(a.id).unwrap());
        assert!(host.is_window_available(b.id).unwrap());
        assert_eq!(
            fake.0.lock().unwrap().calls,
            ["fork:1", "fork:2", "detach:1"]
        );
        // The Global's clone, not a window's lifetime, owns the endpoint.
        application = host.clone();
    }
    assert_eq!(application.window_count().unwrap(), 1);
    assert_eq!(fake.0.lock().unwrap().calls.len(), 3);
}
#[tokio::test(flavor = "current_thread")]
async fn last_window_cancel_leaves_all_owners_untouched_then_stop_is_confirmed() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let window = host.open_window().await.unwrap();
    let decision = host
        .request_close(CloseTarget::Window(window.id), 2)
        .unwrap();
    let CloseDecision::Confirm(confirmation) = decision else {
        panic!("must confirm")
    };
    assert_eq!(confirmation.terminal_count, 2);
    drop(confirmation);
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1"]);
    let ticket = approved(
        host.request_close(CloseTarget::Window(window.id), 2)
            .unwrap(),
    );
    let closed = host.finish_close(ticket).await.unwrap();
    assert!(closed.app_stopped);
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1", "detach:1", "stop"]);
    assert_eq!(host.window_count().unwrap(), 0);
    assert!(host.open_window().await.is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn quit_checks_every_window_including_hidden_dirty_file_and_pending_operations() {
    let host = AppHost::with_backend(Fake::default());
    let a = host.open_window().await.unwrap();
    let b = host.open_window().await.unwrap();
    for guard in [
        WindowCloseGuard::DirtyFile,
        WindowCloseGuard::FileOperation,
        WindowCloseGuard::HostOperation,
    ] {
        host.set_guard(b.id, guard).unwrap();
        let CloseDecision::Blocked(blocked) = host.request_close(CloseTarget::Quit, 0).unwrap()
        else {
            panic!("must block")
        };
        assert_eq!(blocked, vec![(b.id, guard)]);
        // A different clean window can still detach independently.
        assert!(matches!(
            host.request_close(CloseTarget::Window(a.id), 2).unwrap(),
            CloseDecision::Ready(_)
        ));
    }
    host.set_guard(b.id, WindowCloseGuard::Ready).unwrap();
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert_eq!(
        host.finish_close(ticket).await.unwrap().windows,
        [a.id, b.id]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn one_restart_confirmation_reserves_and_closes_every_window() {
    let backend = Fake::default();
    let host = AppHost::with_backend(backend.clone());
    let a = host.open_window().await.unwrap();
    let b = host.open_window().await.unwrap();
    let CloseDecision::Confirm(confirmation) = host.request_close(CloseTarget::Quit, 2).unwrap() else {
        panic!("restart needs one confirmation for all shells");
    };
    assert_eq!(confirmation.terminal_count, 2);
    assert_eq!(backend.0.lock().unwrap().calls, ["fork:1", "fork:2"]);
    let ticket = confirmation.approve();
    assert_eq!(host.begin_close(&ticket).await.unwrap(), [a.id, b.id]);
    assert_eq!(backend.0.lock().unwrap().calls, ["fork:1", "fork:2"]);
    let closed = host.finish_close(ticket).await.unwrap();
    assert_eq!(closed.windows, [a.id, b.id]);
    assert!(closed.app_stopped);
    assert_eq!(backend.0.lock().unwrap().calls, ["fork:1", "fork:2", "detach:1", "detach:2", "stop"]);
}
#[path = "app_host_quit_fault_tests.rs"]
mod quit_fault;

#[tokio::test(flavor = "current_thread")]
async fn failed_release_does_not_stop_or_remove_window() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let window = host.open_window().await.unwrap();
    fake.0.lock().unwrap().fail_detach = true;
    let ticket = approved(
        host.request_close(CloseTarget::Window(window.id), 0)
            .unwrap(),
    );
    assert!(host.finish_close(ticket).await.is_err());
    assert_eq!(host.window_count().unwrap(), 1);
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1", "detach:1"]);
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_wait_does_not_release_application_before_actual_stop_completion() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let window = host.open_window().await.unwrap();
    fake.0.lock().unwrap().wait_stop = true;
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    {
        let mut close = pin!(host.finish_close(ticket.clone()));
        poll_pending(close.as_mut());
    }
    assert_eq!(host.window_count().unwrap(), 1);
    assert!(host.is_closing(window.id).unwrap());
    assert!(host.open_window().await.is_err());
    assert!(
        host.set_guard(window.id, WindowCloseGuard::DirtyFile)
            .is_err()
    );
    assert!(host.abort_close(&ticket).is_err());
    fake.0.lock().unwrap().wait_stop = false;
    host.finish_close(ticket).await.unwrap();
    // Both the accepted Stop and completed detaches survive a cancelled wait.
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1", "detach:1", "stop"]);
}
#[tokio::test(flavor = "current_thread")]
async fn reservation_freezes_membership_during_input_drain_and_can_abort_before_detach() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let a = host.open_window().await.unwrap();
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.is_window_available(a.id).unwrap());
    assert_eq!(host.begin_close(&ticket).await.unwrap(), [a.id]);
    assert!(!host.is_window_available(a.id).unwrap());
    assert!(host.open_window().await.is_err());
    assert!(host.is_closing(a.id).unwrap());
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1"]);
    host.abort_close(&ticket).unwrap();
    assert!(!host.is_closing(a.id).unwrap());
    assert!(host.finish_close(ticket).await.is_err());
    assert!(host.open_window().await.is_ok());
}
#[tokio::test(flavor = "current_thread")]
async fn bounded_window_clients_and_foreign_tickets_are_rejected() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    for _ in 0..MAX_WINDOWS {
        host.open_window().await.unwrap();
    }
    assert!(host.open_window().await.is_err());
    assert_eq!(fake.0.lock().unwrap().next, MAX_WINDOWS);
    let foreign = AppHost::with_backend(Fake::default());
    let ticket = approved(foreign.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.recheck_close(&ticket, 0).is_err());
    assert!(host.finish_close(ticket).await.is_err());
}
