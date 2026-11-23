use super::*;

#[tokio::test(flavor = "current_thread")]
async fn a_host_that_ended_on_its_own_needs_no_stop_and_admits_no_window() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    host.open_window().await.unwrap();
    host.host_ended();
    assert!(host.is_stopped().unwrap());
    assert!(host.stopped_after_operations().await.unwrap());
    assert!(host.open_window().await.is_err());
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1"]);
}

#[tokio::test(flavor = "current_thread")]
async fn cleanup_stops_an_idle_host_once() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    host.cleanup().await.unwrap();
    host.cleanup().await.unwrap();
    assert!(host.is_stopped().unwrap());
    assert_eq!(fake.0.lock().unwrap().calls, ["stop"]);
}

#[tokio::test(flavor = "current_thread")]
async fn zero_windows_with_ready_owner_and_failed_stop_is_not_stopped() {
    let fake = Fake::default();
    fake.0.lock().unwrap().fail_stop = true;
    let host = AppHost::with_backend(fake.clone());
    let application = host.clone();
    assert_eq!(host.window_count().unwrap(), 0);
    assert!(!application.is_stopped().unwrap());
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.finish_close(ticket).await.is_err());
    assert!(!application.is_stopped().unwrap());
    assert_eq!(host.window_count().unwrap(), 0);

    fake.0.lock().unwrap().fail_stop = false;
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.finish_close(ticket).await.unwrap().app_stopped);
    assert!(application.is_stopped().unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn zero_windows_with_cancelled_pending_stop_is_not_stopped() {
    let fake = Fake::default();
    fake.0.lock().unwrap().wait_stop = true;
    let host = AppHost::with_backend(fake.clone());
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    {
        let mut close = std::pin::pin!(host.finish_close(ticket.clone()));
        poll_pending(close.as_mut());
        assert!(!host.is_stopped().unwrap());
    }
    assert_eq!(host.window_count().unwrap(), 0);
    assert!(!host.is_stopped().unwrap());
    assert!(host.abort_close(&ticket).is_err());
    fake.0.lock().unwrap().wait_stop = false;
    host.finish_close(ticket).await.unwrap();
    assert!(host.is_stopped().unwrap());
    assert_eq!(fake.0.lock().unwrap().calls, ["stop"]);
}

#[tokio::test(flavor = "current_thread")]
async fn orphan_registrations_and_secondary_close_do_not_mark_application_stopped() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let (a, b) = {
        // The fake window handles leave scope. There are no native OS windows.
        let a = host.open_window().await.unwrap();
        let b = host.open_window().await.unwrap();
        (a.id, b.id)
    };
    assert_eq!(host.window_count().unwrap(), 2);
    assert!(!host.is_stopped().unwrap());
    let ticket = approved(host.request_close(CloseTarget::Window(a), 0).unwrap());
    assert!(!host.finish_close(ticket).await.unwrap().app_stopped);
    assert!(!host.is_stopped().unwrap());
    fake.0.lock().unwrap().fail_stop = true;
    let ticket = approved(host.request_close(CloseTarget::Window(b), 0).unwrap());
    assert!(host.finish_close(ticket).await.is_err());
    assert!(!host.is_stopped().unwrap());
    assert_eq!(host.window_count().unwrap(), 1);
    fake.0.lock().unwrap().fail_stop = false;
    let ticket = approved(host.request_close(CloseTarget::Window(b), 0).unwrap());
    host.finish_close(ticket).await.unwrap();
    assert!(host.is_stopped().unwrap());
}

#[test]
fn poisoned_registry_returns_error_instead_of_authorizing_quit() {
    let host = AppHost::with_backend(Fake::default());
    let shared = host.shared.clone();
    let result = std::thread::spawn(move || {
        let _state = shared.state.lock().unwrap();
        panic!("controlled registry poison");
    })
    .join();
    assert!(result.is_err());
    assert!(host.is_stopped().is_err());
}
