use super::{HostClosed, LocalClient};
use crate::{local_api::LocalApiRequest, tests::*, OwnerDispatcher, Service};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Panics in the owner's next pass once cued.
struct PanicOnCue(Arc<AtomicBool>);
impl OwnerDispatcher for PanicOnCue {
    fn dispatch_one(&mut self, _service: &mut Service) {
        if self.0.load(Ordering::Acquire) {
            panic!("injected owner panic");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_owner_panic_ends_the_host_and_its_clients_see_it_lost() {
    let data = scratch();
    let (helper, digest) = helper();
    let cue = Arc::new(AtomicBool::new(false));
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (ready, started) = tokio::sync::oneshot::channel();
            let directory = data.path().to_owned();
            let executable = helper.clone();
            let dispatcher = PanicOnCue(cue.clone());
            let mut host = tokio::task::spawn_local(async move {
                super::serve_with_ready(&directory, &executable, digest, dispatcher, move |_| {
                    let _ = ready.send(());
                })
                .await
            });
            tokio::select! {
                result = &mut host => panic!("host stopped before readiness: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(60), started) => {
                    ready.expect("host readiness").expect("readiness callback");
                }
            }
            let client = LocalClient::connect(data.path(), &helper, digest)
                .await
                .expect("client");
            client
                .execute(LocalApiRequest::Snapshot {})
                .await
                .expect("a live owner answers");
            cue.store(true, Ordering::Release);
            // The request wakes the owner into the panicking pass; it need not succeed.
            let _ = client.execute(LocalApiRequest::Snapshot {}).await;
            let closed = tokio::time::timeout(Duration::from_secs(15), client.closed())
                .await
                .expect("the host closes its clients' lifetimes promptly");
            assert_eq!(
                closed,
                HostClosed::Lost,
                "a dead owner is not an orderly stop"
            );
            let ended = tokio::time::timeout(Duration::from_secs(45), host)
                .await
                .expect("the host exits instead of serving failures")
                .expect("host task");
            assert!(ended.is_err(), "an owner death is a host failure");
        })
        .await;
}

/// Blocks the owner's next pass for `STALL` once cued, like a stalled disk write.
struct StallOnCue(Arc<AtomicBool>);
const STALL: Duration = Duration::from_secs(25);
impl OwnerDispatcher for StallOnCue {
    fn dispatch_one(&mut self, _service: &mut Service) {
        if self.0.swap(false, Ordering::AcqRel) {
            std::thread::sleep(STALL);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_stalled_owner_pass_never_ends_the_host() {
    assert!(STALL > crate::owner::SLOW_PASS);
    let data = scratch();
    let (helper, digest) = helper();
    let cue = Arc::new(AtomicBool::new(false));
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (ready, started) = tokio::sync::oneshot::channel();
            let directory = data.path().to_owned();
            let executable = helper.clone();
            let dispatcher = StallOnCue(cue.clone());
            let mut host = tokio::task::spawn_local(async move {
                super::serve_with_ready(&directory, &executable, digest, dispatcher, move |_| {
                    let _ = ready.send(());
                })
                .await
            });
            tokio::select! {
                result = &mut host => panic!("host stopped before readiness: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(60), started) => {
                    ready.expect("host readiness").expect("readiness callback");
                }
            }
            let client = LocalClient::connect(data.path(), &helper, digest)
                .await
                .expect("client");
            cue.store(true, Ordering::Release);
            while cue.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            // The stall began. This request waits it out on the same host.
            let waited = std::time::Instant::now();
            tokio::select! {
                result = &mut host => panic!("a stalled owner ended the host: {result:?}"),
                closed = client.closed() => panic!("a stalled owner closed the client: {closed:?}"),
                answer = client.execute(LocalApiRequest::Snapshot {}) => {
                    answer.expect("the owner answers after its stall");
                }
            }
            assert!(waited.elapsed() > crate::owner::SLOW_PASS);
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert!(!host.is_finished());
        })
        .await;
}
