use super::*;
use crate::host_client::HostEndpointConfig;
#[tokio::test(flavor = "current_thread")]
async fn init_failure_cleanup_retains_exact_host_on_stop_failure_then_preserves_whole_report() {
    let fake = Fake::default();
    let report = ShutdownOutcome {
        activity: ActivityShutdownState::DirtyAfterOwnershipLoss {
            pending_sessions: 3,
        },
        presentation: shellfox_service::PresentationShutdownState::NoKnownFailure,
    };
    {
        let mut state = fake.0.lock().unwrap();
        state.fail_stop = true;
        state.shutdown_outcome = Some(report.clone());
    }
    let host = AppHost::with_backend(fake.clone());
    let retained = host.clone();
    assert!(host.cleanup_prestarted().await.is_err());
    assert!(
        host.same_owner(&retained)
            && host.cleanup_retained().unwrap()
            && !host.is_stopped().unwrap()
    );
    assert!(host.open_window().await.is_err());
    fake.0.lock().unwrap().fail_stop = false;
    assert_eq!(
        host.cleanup_prestarted().await.unwrap(),
        Some(report.clone())
    );
    assert!(retained.is_stopped().unwrap());
    assert_eq!(retained.shutdown_outcome().unwrap(), Some(report));
    assert!(
        !fake
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|call| call == "status")
    ); // no invented/probed zero.
}
#[tokio::test(flavor = "current_thread")]
async fn registered_or_reserved_window_use_rejects_prestart_cleanup_without_stop() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    host.open_window().await.unwrap();
    assert!(host.cleanup_prestarted().await.is_err());
    assert_eq!(fake.0.lock().unwrap().calls, ["fork:1"]);
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    host.begin_close(&ticket).await.unwrap();
    assert!(host.cleanup_prestarted().await.is_err());
    assert!(fake.0.lock().unwrap().calls.is_empty());
    host.abort_close(&ticket).unwrap();
    host.cleanup_prestarted().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_unwindowed_cleanup_fences_new_windows_and_resumes_same_stop() {
    let fake = Fake::default();
    fake.0.lock().unwrap().wait_stop = true;
    let host = AppHost::with_backend(fake.clone());
    {
        let mut wait = pin!(host.cleanup_prestarted());
        poll_pending(wait.as_mut());
    }
    assert!(host.cleanup_retained().unwrap() && !host.is_stopped().unwrap());
    assert!(host.open_window().await.is_err());
    fake.0.lock().unwrap().wait_stop = false;
    host.cleanup_prestarted().await.unwrap();
    assert_eq!(fake.0.lock().unwrap().calls, ["stop"]);
}
#[test]
fn wrong_existing_global_identity_rejects_without_losing_candidate() {
    let config = HostEndpointConfig {
        data_dir: std::path::PathBuf::from("/explicit/fixture"),
        helper: std::path::PathBuf::from("/explicit/helper"),
        helper_sha256: [7; 32],
    };
    let original = AppHost::with_backend(Fake::default());
    let candidate = AppHost::with_backend(Fake::default());
    assert!(adapter::validate_prestarted(&config, &original, &config, &original.clone()).is_ok());
    assert!(adapter::validate_prestarted(&config, &original, &config, &candidate).is_err());
    assert!(!candidate.is_stopped().unwrap()); // caller still owns it for explicit cleanup.
}
