use super::*;
use crate::settings::remote_peer_row::PeerRow;
use shellfox_protocol::CancellationToken;
use std::{future::pending, sync::Mutex};

#[test]
fn navigation_cancels_the_scope_used_by_delayed_connect_receipts() {
    let mut scope = Scope::default();
    let old_generation = scope.generation;
    let lifetime = scope.lifetime(old_generation).unwrap();
    scope.dismiss();
    assert!(lifetime.is_cancelled());
    assert!(scope.lifetime(old_generation).is_none());
    assert!(!scope.record_failure(old_generation, "delayed-attempt".into()));
    assert!(!scope.lifetime(scope.generation).unwrap().is_cancelled());
}

#[test]
fn pre_receipt_connecting_row_has_no_cancel_or_other_action() {
    let (state, _) = fixture();
    let scope = Scope::default();
    let mut peer = state.peers[0].clone();
    peer.last_error = Some("previous failure".into());
    for remembered in [false, true] {
        peer.remembered = remembered;
        for snapshot_id in [None, Some(state.attempts[0].id.as_str())] {
            let row = PeerRow::new(true, &peer, snapshot_id, &scope);
            assert_eq!(row.status, "Connecting…");
            assert!(row.action.is_none());
            assert!(!row.forget);
        }
    }
}

#[test]
fn reopened_pending_row_cancels_displayed_uuid_then_offers_a_fresh_connect() {
    let (mut state, _) = fixture();
    let scope = Scope::default();
    let peer = &mut state.peers[0];
    let without_id = PeerRow::new(false, peer, None, &scope);
    assert!(without_id.action.is_none());
    assert!(!without_id.forget);

    let id = &state.attempts[0].id;
    let pending = PeerRow::new(false, peer, Some(id), &scope);
    assert!(!pending.forget);
    assert!(matches!(
        pending.action,
        Some((key, "Cancel request", remote::Operation::CancelPair(value)))
            if value == *id && key == format!("cancel-peer-{id}")
    ));
    let replacement = PeerRow::new(
        false,
        peer,
        Some("cb814196-cca7-42c3-8165-3ace619f910f"),
        &scope,
    );
    assert_ne!(replacement.action.unwrap().0, format!("cancel-peer-{id}"));

    for connection in [Connection::Disconnected, Connection::Rejected] {
        peer.connection = connection;
        assert!(matches!(
            PeerRow::new(false, peer, None, &scope).action,
            Some((_, "Request again", remote::Operation::Connect(value))) if value == peer.id
        ));
    }
}

#[test]
fn connected_row_replaces_cancel_with_normal_disconnect() {
    let (mut state, _) = fixture();
    let peer = &mut state.peers[0];
    peer.connection = Connection::Connected;
    let row = PeerRow::new(false, peer, None, &Scope::default());
    assert_eq!(row.status, "Connected");
    assert!(matches!(
        row.action,
        Some((_, "Disconnect", remote::Operation::Disconnect(value))) if value == peer.id
    ));
}

fn fixture() -> (api::RemoteState, ConnectResult) {
    let id = "877cf335-6f00-4882-80e0-40f6d2be6718".parse().unwrap();
    let result = ConnectResult::Created(id);
    (
        api::RemoteState {
            enabled: true,
            hostname: "fixture".into(),
            status: api::NetworkStatus::Online,
            login_url: None,
            error: None,
            pending: vec![],
            approvals: vec![],
            limitations: vec![],
            peers: vec![api::Peer {
                id: "peer".into(),
                hostname: "Peer".into(),
                addresses: vec![],
                remembered: true,
                host_confirmed: true,
                online: true,
                connection: Connection::AwaitingApproval,
                last_error: None,
            }],
            attempts: vec![api::RemoteAttempt {
                id: id.to_string(),
                kind: api::RemoteAttemptKind::Connect {
                    peer_id: "peer".into(),
                },
            }],
        },
        result,
    )
}

#[test]
fn existing_and_mismatched_receipts_never_own_the_attempt() {
    let (state, created) = fixture();
    let now = Instant::now();
    let token = CancellationToken::new();
    assert!(
        Watch::new(
            &state,
            "peer".into(),
            ConnectResult::Existing,
            now,
            token.clone()
        )
        .is_none()
    );
    assert!(Watch::new(&state, "another-peer".into(), created, now, token.clone()).is_none());
    let watch = Watch::new(&state, "peer".into(), created, now, token).unwrap();
    assert_eq!(watch.plan(&state, now), Poll::Continue);
}

#[test]
fn delayed_reads_cannot_continue_after_deadline_or_closed_scope() {
    let (state, created) = fixture();
    let started = Instant::now();
    let token = CancellationToken::new();
    let watch = Watch::new(&state, "peer".into(), created, started, token.clone()).unwrap();
    assert_eq!(
        watch.plan(&state, started + LIFETIME - INTERVAL),
        Poll::Continue
    );
    assert_eq!(watch.plan(&state, started + LIFETIME), Poll::Stopped);
    assert_eq!(
        watch.plan(&state, started + Duration::from_secs(301)),
        Poll::Stopped
    );
    token.cancel();
    assert_eq!(watch.plan(&state, started + INTERVAL), Poll::Stopped);
    assert!(!watch.permit.active_at(started + INTERVAL));
}

#[test]
fn uncertain_continue_stops_this_uuid_and_does_not_replay_or_treat_denial_as_retryable() {
    let (mut state, created) = fixture();
    let now = Instant::now();
    let scope = Scope::default();
    let watch = Watch::new(
        &state,
        "peer".into(),
        created,
        now,
        scope.lifetime(scope.generation).unwrap(),
    )
    .unwrap();
    assert_eq!(watch.plan(&state, now), Poll::Continue);
    state.peers[0].connection = Connection::Rejected;
    assert_eq!(watch.plan(&state, now + INTERVAL), Poll::Finished);
    state.peers[0].connection = Connection::AwaitingApproval;
    assert!(watch.accept(Err(anyhow::anyhow!("reply lost"))).is_err());
    assert_eq!(watch.plan(&state, now + 2 * INTERVAL), Poll::Stopped);
    assert!(!scope.lifetime(scope.generation).unwrap().is_cancelled());
    assert_eq!(watch.id, state.attempts[0].id);
}

struct FailingCheck {
    state: Option<api::RemoteState>,
    requests: Mutex<Vec<api::Request>>,
}

impl api::SettingsServices for FailingCheck {
    fn request(&self, request: api::Request) -> api::Reply<'_> {
        let response = match (&request, &self.state) {
            (api::Request::RemoteState, Some(state)) => Ok(api::Response::Remote(state.clone())),
            (api::Request::RemoteState, None) => Err(anyhow::anyhow!("State read failed")),
            (api::Request::ContinuePair { .. }, Some(_)) => {
                Err(anyhow::anyhow!("Queue full before admission"))
            }
            _ => panic!("Unexpected request"),
        };
        let mut requests = self.requests.lock().unwrap();
        requests.push(request);
        assert!(requests.len() <= if self.state.is_some() { 2 } else { 1 });
        Box::pin(async move { response })
    }
}

async fn failing_watch(scope: &mut Scope, services: &FailingCheck) -> String {
    let (state, created) = fixture();
    let generation = scope.generation;
    let watch = Watch::new(
        &state,
        "peer".into(),
        created,
        Instant::now(),
        scope.lifetime(generation).unwrap(),
    )
    .unwrap();
    let operations = operations::Operations::default();
    let result = watch
        .run(
            services,
            |delay| async move {
                if delay > INTERVAL {
                    pending::<()>().await;
                }
            },
            |_| Some(operations.begin()),
        )
        .await;
    assert!(result.is_err());
    assert!(!operations.pending());
    assert!(scope.record_failure(generation, watch.id.clone()));
    watch.id
}

#[tokio::test]
async fn pre_admission_continue_failure_shows_stopped_without_retrying() {
    let (mut state, _) = fixture();
    let services = FailingCheck {
        state: Some(state.clone()),
        requests: Mutex::default(),
    };
    let mut scope = Scope::default();
    let id = failing_watch(&mut scope, &services).await;
    assert!(matches!(
        services.requests.lock().unwrap().as_slice(),
        [api::Request::RemoteState, api::Request::ContinuePair { id: sent, .. }] if sent == &id
    ));

    scope.dismiss();
    let peer = &state.peers[0];
    let row = PeerRow::new(false, peer, Some(&id), &scope);
    assert_eq!(
        row.status,
        "Automatic checking stopped. Cancel the request and try again."
    );
    assert!(matches!(
        row.action,
        Some((_, "Cancel request", remote::Operation::CancelPair(value))) if value == id
    ));

    let replacement = "cb814196-cca7-42c3-8165-3ace619f910f";
    let row = PeerRow::new(false, peer, Some(replacement), &scope);
    assert_eq!(row.status, "Waiting for approval");
    state.attempts.clear();
    state.peers[0].connection = Connection::Disconnected;
    scope.retain_attempts(&state);
    assert!(!scope.stopped(&id));
    assert!(matches!(
        PeerRow::new(false, &state.peers[0], None, &scope).action,
        Some((_, "Request again", remote::Operation::Connect(_)))
    ));
}

#[tokio::test]
async fn state_read_failure_marks_the_attempt_without_continuing() {
    let services = FailingCheck {
        state: None,
        requests: Mutex::default(),
    };
    let mut scope = Scope::default();
    let id = failing_watch(&mut scope, &services).await;
    assert!(scope.stopped(&id));
    assert!(matches!(
        services.requests.lock().unwrap().as_slice(),
        [api::Request::RemoteState]
    ));
}
