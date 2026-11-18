use super::*;

#[test]
fn cancelled_mutation_keeps_cycle_connected_and_does_not_retry() {
    let fake = Fake::default();
    let client = connected(&fake);
    let before = client.state();
    fake.queue(Response::Error(TransportError::Cancelled));
    let request = CreateSession {
        title: None,
        cwd: "/tmp".into(),
        request_id: ID.parse().unwrap(),
    };
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    let after = client.state();
    assert_eq!(after.status, ConnectionStatus::Connected);
    assert_eq!(after.connection_id, before.connection_id);
    assert_eq!(after.permission, Permission::Approved);
    assert_eq!(after.retry_after, None);
    assert_eq!(after.last_error, None);
    assert_eq!(fake.calls(), 2);
    fake.bytes(SNAPSHOT);
    block_on(client.execute(&GetSnapshot {})).unwrap();
    assert_eq!(fake.calls(), 3);
}
#[test]
fn cancelled_poll_does_not_cancel_other_admitted_request_or_require_pairing() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Hold);
    let request = GetSnapshot {};
    let mut other = Box::pin(client.execute(&request));
    start(other.as_mut());
    fake.queue(Response::Error(TransportError::Cancelled));
    assert!(matches!(
        block_on(client.poll()),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert!(!fake.0.lock().unwrap().1[1].limits.cancel.is_cancelled());
    assert_eq!(client.state().status, ConnectionStatus::Connected);
    fake.bytes(EVENTS);
    block_on(client.poll()).unwrap();
    drop(other);
    fake.bytes(SNAPSHOT);
    block_on(client.execute(&request)).unwrap();
}
#[test]
fn cancelled_intent_after_cycle_loss_still_returns_stale_connection() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Hold);
    let request = GetSnapshot {};
    let mut pending = Box::pin(client.execute(&request));
    start(pending.as_mut());
    client.reconnect(ClientId::new_v4());
    assert!(matches!(
        block_on(pending),
        Err(ClientError::StaleConnection)
    ));
    assert_eq!(client.state().status, ConnectionStatus::Reconnecting);
}
