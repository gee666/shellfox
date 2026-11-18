use super::*;
#[test]
fn same_cycle_cancel_keeps_other_operations_and_binding_alive_without_retry() {
    let fake = Fake::default();
    let client = connected(&fake);
    let binding = client.binding().unwrap();
    fake.queue(Response::Wait);
    let request = wire::GetSnapshot::from(GetSnapshot {});
    let mut other = Box::pin(client.execute(&request));
    start(other.as_mut());
    fake.queue(Response::Error(TransportError::Cancelled));
    assert!(matches!(
        block_on(client.poll()),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert_eq!(client.binding(), Some(binding.clone()));
    assert!(!fake.0.lock().unwrap().1[2].limits.cancel.is_cancelled());
    fake.queue(Response::Error(TransportError::Cancelled));
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert_eq!(client.status(), Status::Connected);
    drop(other);
    fake.bytes("snapshot");
    block_on(client.execute(&request)).unwrap();
    assert_eq!(fake.count(), 6);
}
#[test]
fn drop_guards_release_eight_operation_permits_and_one_poll_not_go_transport_budgets() {
    let fake = Fake::default();
    let client = connected(&fake);
    let request = wire::GetSnapshot::from(GetSnapshot {});
    let mut pending = Vec::new();
    for _ in 0..8 {
        fake.queue(Response::Wait);
        let mut work = Box::pin(client.execute(&request));
        start(work.as_mut());
        pending.push(work);
    }
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::Busy)
    ));
    fake.queue(Response::Wait);
    let mut poll = Box::pin(client.poll());
    start(poll.as_mut());
    assert!(matches!(block_on(client.poll()), Err(ClientError::Busy)));
    drop(poll);
    drop(pending);
    assert_eq!(client.status(), Status::Connected);
    fake.bytes("events");
    block_on(client.poll()).unwrap();
    fake.bytes("snapshot");
    block_on(client.execute(&request)).unwrap();
    assert!(fake.0.lock().unwrap().1[2..11]
        .iter()
        .all(|v| v.limits.cancel.is_cancelled()));
}
#[test]
fn dropped_pair_and_discovery_do_not_leave_busy_flags_or_start_automatic_requests() {
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.queue(Response::Wait);
    let mut discovery = Box::pin(client.discover(FallbackPolicy::Never));
    start(discovery.as_mut());
    drop(discovery);
    fake.bytes("discovery");
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    fake.queue(Response::Wait);
    let mut pair = Box::pin(client.pair(id(3)));
    start(pair.as_mut());
    drop(pair);
    assert_eq!(client.status(), Status::Reconnecting);
    fake.bytes("pair-selection");
    block_on(client.pair(id(3))).unwrap();
    assert_eq!(fake.count(), 4);
}
#[test]
fn reconnect_fences_late_responses_and_never_retries_uncertain_mutation() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Wait);
    let request = wire::GetSnapshot::from(GetSnapshot {});
    let mut old = Box::pin(client.execute(&request));
    start(old.as_mut());
    client.reconnect(id(9));
    assert!(matches!(block_on(old), Err(ClientError::StaleConnection)));
    assert!(client.binding().is_none());
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Error(TransportError::Deadline));
    let request = wire::RenameSession::from(RenameSession {
        session_id: "90000000-0000-4000-8000-000000000001".parse().unwrap(),
        title: "new".into(),
    });
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::Transport(TransportError::Deadline))
    ));
    assert_eq!(fake.count(), 3);
    assert!(client.binding().is_none());
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::NotConnected)
    ));
    assert_eq!(fake.count(), 3);
}
#[test]
fn dropped_disconnect_cancels_its_native_wait_without_host_retry() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Wait);
    let mut pending = Box::pin(client.disconnect());
    start(pending.as_mut());
    assert!(client.binding().is_none());
    drop(pending);
    assert!(fake.0.lock().unwrap().1[2].limits.cancel.is_cancelled());
    assert_eq!(fake.count(), 3);
}
#[test]
fn disconnect_withdraws_only_attempt_or_connection_never_host_or_raw_source() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.bytes("disconnected");
    block_on(client.disconnect()).unwrap();
    let calls = &fake.0.lock().unwrap().1;
    assert_eq!(calls[2].endpoint, Endpoint::DisconnectV2);
    assert_eq!(calls[2].body, value("disconnect-connection"));
    assert!(client.binding().is_none());
    let fake = Fake::default();
    let client = super::client(&fake);
    fake.bytes("discovery");
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
    fake.queue(Response::Wait);
    let mut pair = Box::pin(client.pair(id(3)));
    start(pair.as_mut());
    fake.bytes("disconnected");
    block_on(client.disconnect()).unwrap();
    assert!(matches!(block_on(pair), Err(ClientError::StaleConnection)));
    assert_eq!(
        fake.0.lock().unwrap().1[2].body,
        value("disconnect-attempt")
    );
}
