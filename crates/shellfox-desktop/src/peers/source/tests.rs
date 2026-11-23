use super::*;
use shellfox_service::remote::native::RemoteConnection;
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
#[test]
fn settings_approval_shows_in_sidebar_without_replacing_an_active_window_cycle() {
    use shellfox_service::remote::native::RemoteConnection;
    let id = AuthenticatedPeerId::new("n-approval").unwrap();
    let mut remote = peer(id.as_str());
    let mut state = PeerSources::default();
    remote.connection = RemoteConnection::AwaitingApproval;
    let attempt = uuid::Uuid::from_u128(1);
    remote.attempt_id = Some(attempt);
    state.inventory(&[remote.clone()]);
    assert_eq!(state.peers[&id].phase, Phase::AwaitingApproval);
    assert_eq!(state.peers[&id].owner_pair_attempt, Some(attempt));
    let cycle = state.begin(&id).unwrap();
    remote.attempt_id = Some(uuid::Uuid::from_u128(2));
    state.inventory(&[remote]);
    assert!(state.current(&cycle));
    assert!(state.peers[&id].pending);
    assert_eq!(state.peers[&id].phase, Phase::AwaitingApproval);
    assert_eq!(state.peers[&id].owner_pair_attempt, None);
}

#[test]
fn a_retired_settings_attempt_cannot_cancel_a_later_attempt() {
    use shellfox_service::remote::native::RemoteConnection;
    let id = AuthenticatedPeerId::new("n-work").unwrap();
    let mut remote = peer(id.as_str());
    remote.connection = RemoteConnection::Connecting;
    remote.attempt_id = Some(uuid::Uuid::from_u128(1));
    let mut state = PeerSources::default();
    state.inventory(&[remote.clone()]);
    assert_eq!(state.peers[&id].owner_pair_attempt, remote.attempt_id);
    state.retire(&id).unwrap();
    remote.attempt_id = Some(uuid::Uuid::from_u128(2));
    state.inventory(&[remote]);
    assert_eq!(
        state.peers[&id].connection_action(),
        Some(ConnectionAction::Cancel(uuid::Uuid::from_u128(2)))
    );
}

#[test]
fn workspace_retry_only_cancels_its_displayed_owner_receipt() {
    let id = AuthenticatedPeerId::new("n-retry").unwrap();
    let mut remote = peer(id.as_str());
    remote.connection = RemoteConnection::Rejected;
    let old = Uuid::from_u128(1);
    remote.attempt_id = Some(old);
    let mut state = PeerSources::default();
    state.inventory(&[remote.clone()]);
    assert_eq!(
        state.peers[&id].connection_action(),
        Some(ConnectionAction::Retry)
    );

    let cycle = state.begin(&id).unwrap();
    assert_eq!(state.peers[&id].connection_action(), None);
    let retry = Uuid::from_u128(2);
    remote.connection = RemoteConnection::AwaitingApproval;
    remote.attempt_id = Some(retry);
    state.inventory(&[remote.clone()]);
    assert_eq!(state.peers[&id].connection_action(), None);

    state.phase(&cycle, Phase::Disconnected, None);
    state.inventory(&[remote.clone()]);
    assert_eq!(
        state.peers[&id].connection_action(),
        Some(ConnectionAction::Cancel(retry))
    );
    assert!(!state.peers[&id].owner_connected);

    // If approval wins the race, the owner returns Connected without an attempt.
    remote.connection = RemoteConnection::Connected;
    remote.attempt_id = None;
    state.inventory(&[remote]);
    assert_ne!(
        state.peers[&id].connection_action(),
        Some(ConnectionAction::Cancel(retry))
    );
    assert!(state.peers[&id].owner_connected);
}

#[test]
fn folder_request_survives_pairing_but_not_rejection_or_an_offline_peer() {
    let id = AuthenticatedPeerId::new("folder-host").unwrap();
    let mut remote = peer(id.as_str());
    let mut sources = PeerSources::default();
    remote.connection = RemoteConnection::AwaitingApproval;
    sources.inventory(&[remote.clone()]);
    assert!(!sources.peers[&id].creation_connection_failed());

    remote.connection = RemoteConnection::Rejected;
    sources.inventory(&[remote.clone()]);
    assert!(sources.peers[&id].creation_connection_failed());

    let retry = sources.begin(&id).unwrap();
    assert!(!sources.peers[&id].creation_connection_failed());
    sources.phase(&retry, Phase::Disconnected, None);
    remote.connection = RemoteConnection::Disconnected;
    remote.online = false;
    sources.inventory(&[remote]);
    assert!(sources.peers[&id].creation_connection_failed());
}

fn snapshot() -> ManagerSnapshot {
    let raw: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../shellfox-client/tests/fixtures/manager-snapshot-success.json"
    ))
    .unwrap();
    serde_json::from_value(raw["value"].clone()).unwrap()
}
#[test]
fn peer_strip_hides_legacy_and_closed_shells_but_keeps_automation_replay() {
    use shellfox_protocol::{TabLifecycle, TerminalKind};
    let mut tab = snapshot().sessions[0].tabs[0].clone();
    tab.terminal_kind = Some(TerminalKind::Embedded);
    tab.lifecycle = TabLifecycle::Closed;
    tab.profile_id = Some(Some("pwsh".into()));
    assert!(!viewable_tab(&tab));
    tab.profile_id = Some(Some(shellfox_service::AUTOMATION_PROFILE_ID.into()));
    assert!(viewable_tab(&tab));
    tab.terminal_kind = Some(TerminalKind::ExternalLegacy);
    assert!(!viewable_tab(&tab));
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
    let session = snapshot.sessions[0].id;
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
    let session = state.peers[&peer_id].snapshot.as_ref().unwrap().sessions[0].id;
    state.retire(&peer_id).unwrap();
    let current = state.begin(&peer_id).unwrap();
    state.phase(&current, Phase::Connected, None);
    assert!(!state.catalog(&old, snapshot()).unwrap());
    assert!(state.select(&old, session, None).is_err());
    assert!(state.catalog(&current, snapshot()).unwrap());
}
#[test]
fn reconnect_keeps_cached_rows_and_selection_but_rejects_actions_on_old_binding() {
    let peer_id = AuthenticatedPeerId::new("n-reconnect").unwrap();
    let mut state = PeerSources::default();
    state.inventory(&[peer(peer_id.as_str())]);
    let old = state.begin(&peer_id).unwrap();
    state.phase(&old, Phase::Connected, None);
    let mut before = snapshot();
    before.revision = 40;
    let (session, tab) = before
        .sessions
        .first()
        .map(|s| (s.id, s.tabs.first().map(|t| t.id)))
        .unwrap();
    state.catalog(&old, before.clone()).unwrap();
    state.select(&old, session, tab).unwrap();
    state.peers.get_mut(&peer_id).unwrap().home = Some("/fixture".into());
    state.phase(&old, Phase::Reconnecting, Some("disconnected".into()));
    let new = state.begin(&peer_id).unwrap();
    assert!(state.select(&old, session, tab).is_err());
    assert!(state.select(&new, session, tab).is_err());
    let peer = &state.peers[&peer_id];
    assert_eq!(peer.selected_session, Some(session));
    assert_eq!(peer.selected_tab, tab);
    assert_eq!(peer.snapshot.as_ref().unwrap().revision, 40);
    assert_eq!(peer.home.as_deref(), Some("/fixture"));
    before.revision = 2;
    state.phase(&new, Phase::Connected, None);
    assert!(state.catalog(&new, before).unwrap());
    assert_eq!(state.peers[&peer_id].selected_session, Some(session));
}

#[test]
fn explicit_disconnect_restores_only_the_same_viewable_session_and_tab() {
    let peer_id = AuthenticatedPeerId::new("n-return").unwrap();
    let mut sources = PeerSources::default();
    sources.inventory(&[peer(peer_id.as_str())]);
    let old = sources.begin(&peer_id).unwrap();
    let before = snapshot();
    let (id, tab) = before
        .sessions
        .iter()
        .find_map(|session| {
            session
                .tabs
                .iter()
                .find(|tab| viewable_tab(tab))
                .map(|tab| (session.id, tab.id))
        })
        .unwrap();
    sources.phase(&old, Phase::Connected, None);
    sources.catalog(&old, before.clone()).unwrap();
    sources.select(&old, id, Some(tab)).unwrap();
    sources.retire(&peer_id).unwrap();
    let saved = &sources.peers[&peer_id];
    assert_eq!(saved.selected_session, Some(id));
    assert_eq!(saved.selected_tab, Some(tab));
    assert!(saved.snapshot.is_none());
    assert!(sources.select(&old, id, Some(tab)).is_err());

    let new = sources.begin(&peer_id).unwrap();
    sources.phase(&new, Phase::Connected, None);
    sources.catalog(&new, before.clone()).unwrap();
    assert_eq!(sources.peers[&peer_id].selected_tab, Some(tab));

    sources.retire(&peer_id).unwrap();
    let third = sources.begin(&peer_id).unwrap();
    sources.phase(&third, Phase::Connected, None);
    let mut without_tab = before;
    for session in &mut without_tab.sessions {
        session.tabs.retain(|item| item.id != tab);
    }
    sources.catalog(&third, without_tab).unwrap();
    let saved = &sources.peers[&peer_id];
    assert!(saved.selection_committed);
    assert_eq!((saved.selected_session, saved.selected_tab), (None, None));
}

#[test]
fn a_vanished_selected_tab_hands_the_view_to_the_next_live_tab() {
    let peer_id = AuthenticatedPeerId::new("n-vanished").unwrap();
    let mut sources = PeerSources::default();
    sources.inventory(&[peer(peer_id.as_str())]);
    let cycle = sources.begin(&peer_id).unwrap();
    sources.phase(&cycle, Phase::Connected, None);
    let mut before = snapshot();
    before.revision = 10;
    let (session, tab) = before
        .sessions
        .iter()
        .find_map(|session| {
            session
                .tabs
                .iter()
                .find(|tab| viewable_tab(tab))
                .map(|tab| (session.id, tab.clone()))
        })
        .unwrap();
    let mut next = tab.clone();
    next.id = TabId::new();
    for value in before
        .sessions
        .iter_mut()
        .filter(|value| value.id == session)
    {
        value.tabs.push(next.clone());
    }
    sources.catalog(&cycle, before.clone()).unwrap();
    sources.select(&cycle, session, Some(tab.id)).unwrap();
    let mut after = before;
    after.revision = 11;
    for value in &mut after.sessions {
        value.tabs.retain(|item| item.id != tab.id);
    }
    assert!(sources.catalog(&cycle, after).unwrap());
    let state = &sources.peers[&peer_id];
    assert_eq!(
        (state.selected_session, state.selected_tab),
        (Some(session), Some(next.id))
    );
}
