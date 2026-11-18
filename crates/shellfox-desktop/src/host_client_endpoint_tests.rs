use super::*;
use std::{
    future::Future,
    pin::pin,
    task::{Context as TaskContext, Poll, Waker},
};

fn poll_pending(future: std::pin::Pin<&mut impl Future>) {
    assert!(matches!(
        future.poll(&mut TaskContext::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn nonexistent_root_reaches_native_worker_without_frontend_directory_creation() {
    let scratch =
        std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))
            .unwrap();
    let parent = scratch.join(format!(
        "endpoint-cold-plan-{}",
        shellfox_core::SessionId::new()
    ));
    assert!(!parent.exists());
    let data_dir = parent.join("fresh").join("host");
    let mut endpoint = HostEndpoint::start(HostEndpointConfig {
        data_dir: data_dir.clone(),
        helper: std::env::current_exe().unwrap(),
        helper_sha256: [0; 32],
    })
    .expect("nonexistent root is not an outer startup error");
    assert!(endpoint.ready().await.is_err()); // Wrong digest is rejected purely in the native worker.
    assert_eq!(
        endpoint.startup_outcome(),
        HostEndpointStartupOutcome::NoOwnerClean
    );
    endpoint.shutdown().await.unwrap();
    assert!(!data_dir.exists() && !parent.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_retains_startup_receiver_and_completed_error_is_not_repolled() {
    let (ready_tx, ready) = oneshot::channel();
    let (_, done) = oneshot::channel();
    let (shutdown, mut stopping) = mpsc::channel(1);
    let (_, startup) = watch::channel(HostEndpointStartupOutcome::RetainedCleanup);
    let mut endpoint = HostEndpoint {
        ready: Some(ready),
        done: Some(done),
        shutdown,
        thread: None,
        client: None,
        shutdown_pending: None,
        stop_acknowledged: false,
        startup,
        shutdown_report: watch::channel(None).1,
        endpoint_report: watch::channel(None).1,
        startup_failure: watch::channel(None).1,
    };
    {
        let mut future = pin!(endpoint.ready());
        poll_pending(future.as_mut());
    }
    assert!(endpoint.ready.is_some());
    assert!(
        ready_tx
            .send(Err(anyhow::anyhow!("controlled startup failure")))
            .is_ok()
    );
    assert!(endpoint.ready().await.is_err());
    assert!(endpoint.ready.is_none());
    assert!(endpoint.ready().await.is_err());
    assert_eq!(
        endpoint.startup_outcome(),
        HostEndpointStartupOutcome::RetainedCleanup
    );
    assert!(endpoint.shutdown().await.is_err());
    assert!(stopping.try_recv().is_err()); // No Stop sent to a nonexistent endpoint.
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_shutdown_wait_does_not_resubmit_stop_or_lose_cleanup_confirmation() {
    for initial in [
        HostEndpointStartupOutcome::Ready,
        HostEndpointStartupOutcome::RetainedCleanup,
    ] {
        let (_, ready) = oneshot::channel();
        let (done_tx, done) = oneshot::channel();
        let (shutdown, mut stopping) = mpsc::channel(1);
        let thread = std::thread::spawn(|| ());
        let (startup_tx, startup) = watch::channel(initial);
        let mut endpoint = HostEndpoint {
            ready: Some(ready),
            done: Some(done),
            shutdown,
            thread: Some(thread),
            client: None,
            shutdown_pending: None,
            stop_acknowledged: false,
            startup,
            shutdown_report: watch::channel(None).1,
            endpoint_report: watch::channel(None).1,
            startup_failure: watch::channel(None).1,
        };
        {
            let mut future = pin!(endpoint.shutdown());
            poll_pending(future.as_mut());
        }
        let reply = stopping.try_recv().expect("one explicit Stop request");
        assert!(endpoint.shutdown_pending.is_some());
        reply.send(Ok(())).unwrap();
        {
            let mut future = pin!(endpoint.shutdown());
            poll_pending(future.as_mut()); // Stop replied, but owner/helper teardown not yet confirmed.
        }
        assert!(endpoint.stop_acknowledged);
        assert!(stopping.try_recv().is_err());
        startup_tx.send_replace(HostEndpointStartupOutcome::NoOwnerClean);
        done_tx.send(Ok(())).unwrap();
        endpoint.shutdown().await.unwrap();
        assert!(endpoint.thread.is_none());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn finished_thread_and_ready_error_are_not_an_ownership_proof() {
    let (ready_tx, ready) = oneshot::channel();
    let (done_tx, done) = oneshot::channel();
    let (shutdown, mut stopping) = mpsc::channel(1);
    let (_, startup) = watch::channel(HostEndpointStartupOutcome::RetainedCleanup);
    let thread = std::thread::spawn(|| ());
    thread.join().unwrap(); // Thread completion alone is intentionally insufficient.
    let mut endpoint = HostEndpoint {
        ready: Some(ready),
        done: Some(done),
        shutdown,
        thread: None,
        client: None,
        shutdown_pending: None,
        stop_acknowledged: false,
        startup,
        shutdown_report: watch::channel(None).1,
        endpoint_report: watch::channel(None).1,
        startup_failure: watch::channel(None).1,
    };
    assert!(
        ready_tx
            .send(Err(anyhow::anyhow!("late partial startup failure")))
            .is_ok()
    );
    done_tx.send(Ok(())).unwrap();
    assert!(endpoint.ready().await.is_err());
    assert_eq!(
        endpoint.startup_outcome(),
        HostEndpointStartupOutcome::RetainedCleanup
    );
    assert!(endpoint.shutdown().await.is_err());
    assert!(stopping.try_recv().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_clean_failed_startup_closes_without_sending_stop() {
    let (ready_tx, ready) = oneshot::channel();
    let (done_tx, done) = oneshot::channel();
    let (shutdown, mut stopping) = mpsc::channel(1);
    let (proof, startup) = watch::channel(HostEndpointStartupOutcome::RetainedCleanup);
    let (report, shutdown_report) = watch::channel(None);
    let mut endpoint = HostEndpoint {
        ready: Some(ready),
        done: Some(done),
        shutdown,
        thread: Some(std::thread::spawn(|| ())),
        client: None,
        shutdown_pending: None,
        stop_acknowledged: false,
        startup,
        shutdown_report,
        endpoint_report: watch::channel(None).1,
        startup_failure: watch::channel(None).1,
    };
    assert!(
        ready_tx
            .send(Err(anyhow::anyhow!("preownership validation rejection")))
            .is_ok()
    );
    done_tx
        .send(Err(anyhow::anyhow!(
            "operational error, no cleanup failure"
        )))
        .unwrap();
    assert!(endpoint.ready().await.is_err());
    // A known-loss metadata warning does not imply live process ownership.
    let dirty = shellfox_service::ShutdownOutcome {
        activity: shellfox_service::ActivityShutdownState::DirtyAfterOwnershipLoss {
            pending_sessions: 2,
        },
        presentation: shellfox_service::PresentationShutdownState::NoKnownFailure,
    };
    report.send_replace(Some(dirty.clone()));
    // Only this explicit service lifecycle proof permits normal close.
    proof.send_replace(HostEndpointStartupOutcome::NoOwnerClean);
    endpoint.shutdown().await.unwrap();
    assert_eq!(endpoint.shutdown_outcome(), Some(dirty));
    assert!(endpoint.thread.is_none());
    assert!(stopping.try_recv().is_err());
}
