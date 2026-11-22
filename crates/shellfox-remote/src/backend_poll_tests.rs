use super::tests::request;
use super::*;
use crate::{Approval, ViewPermission};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const NOW: u64 = 1700000000123;

/// Empty polls until `ready`; the first empty poll can also raise the wake
/// itself, which models output landing between the owner poll and the wait.
#[derive(Default)]
struct Host {
    ready: AtomicBool,
    wake_on_first_poll: bool,
    polls: AtomicUsize,
}
impl RemoteHost for Host {
    fn request(&self, command: OwnerCommand) -> crate::channel::OwnerFuture<'_> {
        Box::pin(async move {
            let OwnerCommand::Poll { actor } = command else {
                return Ok(json!({}));
            };
            let ready = self.ready.load(Ordering::SeqCst);
            if self.polls.fetch_add(1, Ordering::SeqCst) == 0 && self.wake_on_first_poll {
                self.ready.store(true, Ordering::SeqCst);
                actor.poll_wake().notify();
            }
            Ok(if ready {
                json!({"events":[{"type":"changed","event":{"revision":1,"reason":"sessions"}}]})
            } else {
                json!({"events":[]})
            })
        })
    }
}
async fn paired(host: Arc<Host>) -> (Arc<Backend>, String, u64, Actor) {
    let control = ApprovalControl::default();
    control
        .install(Approval {
            peer_id: "n-peer".into(),
            hostname: "my laptop".into(),
            approved_at_millis: NOW,
            permission: ViewPermission::Write,
            file_roots: vec![],
        })
        .unwrap();
    let mut backend = Backend::new(host, control, Secret::new("b".repeat(32)).unwrap());
    backend.clock = || NOW;
    backend.handle_at(request("pair"), NOW).await.unwrap();
    let (id, view) = {
        let limits = backend.limits.lock().unwrap();
        let (id, view) = limits.views.iter().next().unwrap();
        (id.clone(), (view.epoch, view.actor.clone()))
    };
    (Arc::new(backend), id, view.0, view.1)
}

#[tokio::test(start_paused = true)]
async fn wake_returns_new_events_without_waiting_for_a_timer() {
    let host = Arc::new(Host::default());
    let (backend, id, epoch, actor) = paired(host.clone()).await;
    let started = tokio::time::Instant::now();
    let wake = actor.poll_wake();
    let poll = tokio::spawn({
        let backend = backend.clone();
        async move { backend.poll(&id, epoch, actor).await }
    });
    while host.polls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    host.ready.store(true, Ordering::SeqCst);
    wake.notify();
    let value = poll.await.unwrap().unwrap();
    assert_eq!(value["events"].as_array().unwrap().len(), 1);
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn wake_between_owner_poll_and_wait_is_not_lost() {
    let host = Arc::new(Host {
        wake_on_first_poll: true,
        ..Host::default()
    });
    let (backend, id, epoch, actor) = paired(host.clone()).await;
    let started = tokio::time::Instant::now();
    // The first poll reports nothing, then raises the wake before any wait.
    let value = backend.poll(&id, epoch, actor).await.unwrap();
    assert_eq!(value["events"].as_array().unwrap().len(), 1);
    assert_eq!(host.polls.load(Ordering::SeqCst), 2);
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn disconnect_ends_the_pending_poll() {
    let host = Arc::new(Host::default());
    let (backend, id, epoch, actor) = paired(host.clone()).await;
    let started = tokio::time::Instant::now();
    let poll = tokio::spawn({
        let backend = backend.clone();
        async move { backend.poll(&id, epoch, actor).await }
    });
    while host.polls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    backend.handle_at(request("disconnect"), NOW).await.unwrap();
    assert!(poll.await.unwrap().is_err());
    assert_eq!(started.elapsed(), Duration::ZERO);
}
