use super::*;
fn peer(id: &str) -> shellfox_service::remote::native::RemotePeer {
    shellfox_service::remote::native::RemotePeer {
        id: id.into(),
        hostname: id.into(),
        addresses: vec![],
        online: true,
        remembered: true,
        host_confirmed: true,
        connection: shellfox_service::remote::native::RemoteConnection::Disconnected,
        last_error: None,
        attempt_id: None,
    }
}
fn snapshot() -> m::Snapshot {
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
    ))
    .unwrap();
    serde_json::from_value(raw["value"].clone()).unwrap()
}
#[test]
fn identical_session_ids_on_different_peers_do_not_alias_local_or_each_other() {
    let a = AuthenticatedPeerId::new("n-home").unwrap();
    let b = AuthenticatedPeerId::new("n-work").unwrap();
    let mut state = PeerSources::default();
    state.inventory(&[peer(a.as_str()), peer(b.as_str())]);
    let ca = state.begin(&a).unwrap();
    let cb = state.begin(&b).unwrap();
    state.phase(&ca, Phase::Connected, None);
    state.phase(&cb, Phase::Connected, None);
    let snapshot = snapshot();
    let session = sessions(&snapshot).next().unwrap().id;
    state.catalog(&ca, snapshot.clone()).unwrap();
    state.catalog(&cb, snapshot).unwrap();
    state.select(&ca, session, None).unwrap();
    assert_eq!(state.selected, Source::Peer(a.clone()));
    assert_eq!(state.selected_peer().unwrap().0, &a);
    assert!(state.peers.get(&b).unwrap().selected_session.is_none());
    state.choose(Source::Local).unwrap();
    assert_eq!(state.peers.get(&a).unwrap().selected_session, Some(session));
    state.select(&cb, session, None).unwrap();
    assert_eq!(state.selected, Source::Peer(b));
}
#[test]
fn returning_to_local_does_not_revalidate_a_held_local_ui_result() {
    let mut state = PeerSources::default();
    let original = state.selection_epoch();
    state
        .choose(Source::Peer(AuthenticatedPeerId::new("n-work").unwrap()))
        .unwrap();
    let foreign = state.selection_epoch();
    assert_ne!(foreign, original);
    state.choose(Source::Local).unwrap();
    assert_ne!(state.selection_epoch(), original);
    let current = state.selection_epoch();
    state.choose(Source::Local).unwrap();
    assert_eq!(state.selection_epoch(), current);
}
#[test]
fn pending_approval_rejection_and_reconnect_never_keep_live_catalog_authority() {
    let id = AuthenticatedPeerId::new("n-work").unwrap();
    let mut state = PeerSources::default();
    assert_eq!(state.selection_epoch(), 1);
    state.choose(Source::Local).unwrap();
    assert_eq!(state.selection_epoch(), 1);
    assert!(!state.inventory_pending);
    state.inventory_pending = true;
    assert!(state.inventory_pending);
    state.inventory(&[peer(id.as_str())]);
    for phase in [
        Phase::AwaitingApproval,
        Phase::Rejected,
        Phase::Reconnecting,
    ] {
        let cycle = state.begin(&id).unwrap();
        assert!(state.phase(&cycle, phase, Some("Host state".into())));
        let peer = &state.peers[&id];
        assert_eq!(peer.phase, phase);
        assert!(peer.snapshot.is_none());
        assert!(peer.origin.is_none());
        assert!(!peer.pending);
        assert!(!state.catalog(&cycle, snapshot()).unwrap());
    }
}
#[test]
fn late_catalog_and_selection_cannot_rebind_after_peer_reconnect() {
    let peer_id = AuthenticatedPeerId::new("n-work").unwrap();
    let mut state = PeerSources::default();
    state.inventory(&[peer(peer_id.as_str())]);
    let old = state.begin(&peer_id).unwrap();
    state.phase(&old, Phase::Connected, None);
    state.catalog(&old, snapshot()).unwrap();
    let session = sessions(state.peers[&peer_id].snapshot.as_ref().unwrap())
        .next()
        .unwrap()
        .id;
    state.retire(&peer_id).unwrap();
    let current = state.begin(&peer_id).unwrap();
    state.phase(&current, Phase::Connected, None);
    assert!(!state.catalog(&old, snapshot()).unwrap());
    assert!(state.select(&old, session, None).is_err());
    assert!(state.catalog(&current, snapshot()).unwrap());
}
#[test]
fn complete_catalog_retains_unavailable_rows_and_does_not_make_fake_sessions() {
    let id = AuthenticatedPeerId::new("n-home").unwrap();
    let mut state = PeerSources::default();
    state.inventory(&[peer(id.as_str())]);
    let cycle = state.begin(&id).unwrap();
    state.phase(&cycle, Phase::Connected, None);
    let snapshot = snapshot();
    assert_eq!(snapshot.entries.len(), 3);
    state.catalog(&cycle, snapshot).unwrap();
    let accepted = state.peers[&id].snapshot.as_ref().unwrap();
    assert_eq!(accepted.coverage.entry_count, 3);
    assert_eq!(sessions(accepted).count(), 2);
    assert!(matches!(
        accepted.entries[2],
        m::CatalogEntry::Unavailable { .. }
    ));
}
