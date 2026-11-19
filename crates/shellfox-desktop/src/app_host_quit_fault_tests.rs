use super::*;

#[tokio::test(flavor = "current_thread")]
async fn stale_approval_cannot_close_a_window_that_became_last_or_dirty() {
    let host = AppHost::with_backend(Fake::default());
    let a = host.open_window().await.unwrap();
    let b = host.open_window().await.unwrap();
    let a_ticket = approved(host.request_close(CloseTarget::Window(a.id), 3).unwrap());
    let b_ticket = approved(host.request_close(CloseTarget::Window(b.id), 3).unwrap());
    host.finish_close(a_ticket).await.unwrap();
    assert!(host.finish_close(b_ticket).await.is_err());
    let ticket = approved(host.request_close(CloseTarget::Window(b.id), 3).unwrap());
    host.set_guard(b.id, WindowCloseGuard::DirtyFile).unwrap();
    assert!(host.finish_close(ticket.clone()).await.is_err());
    assert_eq!(host.window_count().unwrap(), 1);
    assert!(matches!(
        host.recheck_close(&ticket, 3).unwrap(),
        CloseDecision::Blocked(_)
    ));
    host.set_guard(b.id, WindowCloseGuard::Ready).unwrap();
    let CloseDecision::Confirm(refreshed) = host.recheck_close(&ticket, 4).unwrap() else {
        panic!("retry must request approval against the fresh count");
    };
    assert_eq!(refreshed.terminal_count, 4);
    assert!(host.begin_close(&ticket).await.is_err());
    assert!(
        host.finish_close(refreshed.approve())
            .await
            .unwrap()
            .app_stopped
    );
}
#[tokio::test(flavor = "current_thread")]
async fn stop_failure_retains_application_host_windows_and_requires_new_approval() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let window = host.open_window().await.unwrap();
    fake.0.lock().unwrap().fail_stop = true;
    let ticket = approved(host.request_close(CloseTarget::Quit, 1).unwrap());
    assert!(host.finish_close(ticket.clone()).await.is_err());
    assert!(host.cleanup_retained().unwrap());
    assert_eq!(host.window_count().unwrap(), 1);
    assert!(host.open_window().await.is_err());
    assert!(host.finish_close(ticket).await.is_err());
    assert!(matches!(
        host.request_close(CloseTarget::Quit, 1).unwrap(),
        CloseDecision::Confirm(_)
    ));
    fake.0.lock().unwrap().fail_stop = false;
    let ticket = approved(host.request_close(CloseTarget::Quit, 1).unwrap());
    assert_eq!(
        host.finish_close(ticket).await.unwrap().windows,
        [window.id]
    );
    assert!(!host.cleanup_retained().unwrap());
}
