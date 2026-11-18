use super::*;

fn dirty(pending_sessions: usize) -> ShutdownOutcome {
    ShutdownOutcome {
        activity: ActivityShutdownState::DirtyAfterOwnershipLoss { pending_sessions },
        presentation: shellfox_service::PresentationShutdownState::NoKnownFailure,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn dirty_persistence_report_is_retained_without_claiming_live_resources_or_commit() {
    let fake = Fake::default();
    let report = dirty(23);
    fake.0.lock().unwrap().shutdown_outcome = Some(report.clone());
    let host = AppHost::with_backend(fake.clone());
    let observer = host.clone();
    let window = host.open_window().await.unwrap();
    assert_eq!(host.shutdown_outcome().unwrap(), None);
    let ticket = approved(
        host.request_close(CloseTarget::Window(window.id), 0)
            .unwrap(),
    );
    assert!(host.finish_close(ticket).await.unwrap().app_stopped);
    assert!(observer.is_stopped().unwrap());
    assert!(!observer.cleanup_retained().unwrap());
    assert_eq!(observer.shutdown_outcome().unwrap(), Some(report));
    assert_eq!(host.window_count().unwrap(), 0);
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1", "detach:1", "stop"]);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_resource_cleanup_does_not_publish_a_successful_shutdown_report() {
    let fake = Fake::default();
    let report = dirty(11);
    {
        let mut state = fake.0.lock().unwrap();
        state.fail_stop = true;
        state.shutdown_outcome = Some(report.clone());
    }
    let host = AppHost::with_backend(fake.clone());
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.finish_close(ticket).await.is_err());
    assert!(!host.is_stopped().unwrap());
    assert!(host.cleanup_retained().unwrap());
    assert_eq!(host.shutdown_outcome().unwrap(), None);
    fake.0.lock().unwrap().fail_stop = false;
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    host.finish_close(ticket).await.unwrap();
    assert!(host.is_stopped().unwrap());
    assert!(!host.cleanup_retained().unwrap());
    assert_eq!(host.shutdown_outcome().unwrap(), Some(report));
}

#[tokio::test(flavor = "current_thread")]
async fn clean_failed_startup_preserves_dirty_report_without_rpc_stop() {
    let fake = Fake::default();
    let report = dirty(7);
    {
        let mut state = fake.0.lock().unwrap();
        state.fail_fork = true;
        state.boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
        state.shutdown_outcome = Some(report.clone());
    }
    let host = AppHost::with_backend(fake.clone());
    assert!(host.open_window().await.is_err());
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    host.finish_close(ticket).await.unwrap();
    assert!(host.is_stopped().unwrap());
    assert_eq!(host.shutdown_outcome().unwrap(), Some(report));
    assert_eq!(fake.0.lock().unwrap().calls, ["clean-proof"]);
}

#[tokio::test(flavor = "current_thread")]
async fn missing_report_is_not_fabricated_into_committed_metadata() {
    for report in [
        None,
        Some(ShutdownOutcome {
            activity: ActivityShutdownState::Committed,
            presentation: shellfox_service::PresentationShutdownState::NoKnownFailure,
        }),
    ] {
        let fake = Fake::default();
        {
            let mut state = fake.0.lock().unwrap();
            state.boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
            state.shutdown_outcome = report.clone();
        }
        let host = AppHost::with_backend(fake);
        assert_eq!(host.shutdown_outcome().unwrap(), None);
        let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
        host.finish_close(ticket).await.unwrap();
        assert!(host.is_stopped().unwrap());
        assert_eq!(host.shutdown_outcome().unwrap(), report);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn secondary_close_and_cancelled_stop_do_not_publish_report_before_completion() {
    let fake = Fake::default();
    let report = dirty(5);
    fake.0.lock().unwrap().shutdown_outcome = Some(report.clone());
    let host = AppHost::with_backend(fake.clone());
    let a = host.open_window().await.unwrap();
    let b = host.open_window().await.unwrap();
    let ticket = approved(host.request_close(CloseTarget::Window(a.id), 0).unwrap());
    assert!(!host.finish_close(ticket).await.unwrap().app_stopped);
    assert_eq!(host.shutdown_outcome().unwrap(), None);
    fake.0.lock().unwrap().wait_stop = true;
    let ticket = approved(host.request_close(CloseTarget::Window(b.id), 0).unwrap());
    {
        let mut close = std::pin::pin!(host.finish_close(ticket.clone()));
        poll_pending(close.as_mut());
    }
    assert!(!host.is_stopped().unwrap());
    assert_eq!(host.shutdown_outcome().unwrap(), None);
    fake.0.lock().unwrap().wait_stop = false;
    host.finish_close(ticket).await.unwrap();
    assert!(host.is_stopped().unwrap());
    assert_eq!(host.shutdown_outcome().unwrap(), Some(report));
    assert_eq!(
        fake.0.lock().unwrap().calls,
        ["fork:1", "fork:2", "detach:1", "detach:2", "stop"]
    );
}
