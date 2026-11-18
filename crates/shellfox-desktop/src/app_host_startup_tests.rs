use super::*;

#[tokio::test(flavor = "current_thread")]
async fn final_registration_observation_waits_for_original_close_operation() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let operation = host.shared.operation.lock().await;
    let mut observation = std::pin::pin!(host.startup_outcome_after_operations());
    poll_pending(observation.as_mut());
    // Simulate the original endpoint publishing its confirmed outcome before
    // the in-flight close releases its operation guard. No unregister text is proof.
    fake.0.lock().unwrap().boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
    drop(operation);
    assert_eq!(
        observation.await.unwrap(),
        HostEndpointStartupOutcome::NoOwnerClean
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failed_ready_clean_source_proof_allows_zero_window_quit_without_stop_rpc() {
    let fake = Fake::default();
    {
        let mut state = fake.0.lock().unwrap();
        state.fail_fork = true;
        state.boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
    }
    let host = AppHost::with_backend(fake.clone());
    assert!(host.open_window().await.is_err());
    assert_eq!(host.window_count().unwrap(), 0);
    assert_eq!(
        host.startup_outcome().await.unwrap(),
        HostEndpointStartupOutcome::NoOwnerClean
    );
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.finish_close(ticket).await.unwrap().app_stopped);
    assert_eq!(fake.0.lock().unwrap().calls, ["clean-proof"]);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_ready_retained_source_cannot_be_overridden_by_error_text_or_empty_registry() {
    let fake = Fake::default();
    {
        let mut state = fake.0.lock().unwrap();
        state.fail_fork = true;
        state.boot = Some(HostEndpointStartupOutcome::RetainedCleanup);
    }
    let host = AppHost::with_backend(fake.clone());
    // Fake readiness fails with misleading "NoOwnerClean" text in both cases.
    assert!(host.open_window().await.is_err());
    assert_eq!(host.window_count().unwrap(), 0);
    assert_eq!(
        host.startup_outcome().await.unwrap(),
        HostEndpointStartupOutcome::RetainedCleanup
    );
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.finish_close(ticket).await.is_err());
    assert!(host.cleanup_retained().unwrap());
    assert_eq!(fake.0.lock().unwrap().calls, ["retained"]);
    // The same app retains its backend until the source later reports explicit
    // actor/helper/ownership cleanup. No restart, thread-result inference or kill.
    fake.0.lock().unwrap().boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
    let ticket = approved(host.request_close(CloseTarget::Quit, 0).unwrap());
    assert!(host.finish_close(ticket).await.unwrap().app_stopped);
    assert_eq!(fake.0.lock().unwrap().calls, ["retained", "clean-proof"]);
}

#[tokio::test(flavor = "current_thread")]
async fn no_owner_source_proof_does_not_trap_existing_windows_on_dead_client_release() {
    let fake = Fake::default();
    let host = AppHost::with_backend(fake.clone());
    let a = host.open_window().await.unwrap();
    let b = host.open_window().await.unwrap();
    {
        let mut state = fake.0.lock().unwrap();
        state.boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
        state.fail_detach = true;
    }
    let ticket = approved(host.request_close(CloseTarget::Window(a.id), 0).unwrap());
    let closed = host.finish_close(ticket).await.unwrap();
    assert!(!closed.app_stopped);
    assert_eq!(closed.windows, [a.id]);
    assert_eq!(host.window_count().unwrap(), 1);
    let ticket = approved(host.request_close(CloseTarget::Window(b.id), 0).unwrap());
    assert!(host.finish_close(ticket).await.unwrap().app_stopped);
    assert_eq!(
        fake.0.lock().unwrap().calls,
        ["fork:1", "fork:2", "clean-proof"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn close_confirmation_uses_actual_native_owner_count_and_unknown_is_not_zero() {
    let fake = Fake::default();
    fake.0.lock().unwrap().owned_terminals = 7;
    let host = AppHost::with_backend(fake.clone());
    let count = host.owned_terminal_count().await.unwrap();
    assert_eq!(count, 7);
    let CloseDecision::Confirm(confirmation) =
        host.request_close(CloseTarget::Quit, count).unwrap()
    else {
        panic!("actual owners must be confirmed even without GUI views");
    };
    assert_eq!(confirmation.terminal_count, 7);
    drop(confirmation);
    fake.0.lock().unwrap().boot = Some(HostEndpointStartupOutcome::RetainedCleanup);
    assert!(host.owned_terminal_count().await.is_err());
    fake.0.lock().unwrap().boot = Some(HostEndpointStartupOutcome::NoOwnerClean);
    assert_eq!(host.owned_terminal_count().await.unwrap(), 0);
    assert_eq!(fake.0.lock().unwrap().calls, ["status"]);
}
