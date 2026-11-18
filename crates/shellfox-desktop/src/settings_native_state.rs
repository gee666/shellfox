use super::api;
use shellfox_service::remote::native as host;
pub(super) fn project(state: host::RemoteControlState) -> api::RemoteState {
    let status = match state.status {
        host::RemoteNetworkStatus::Disabled => api::NetworkStatus::Disabled,
        host::RemoteNetworkStatus::Starting => api::NetworkStatus::Starting,
        host::RemoteNetworkStatus::LoginRequired => api::NetworkStatus::LoginRequired,
        host::RemoteNetworkStatus::Online => api::NetworkStatus::Online,
        host::RemoteNetworkStatus::Error => api::NetworkStatus::Error,
    };
    let peers = state
        .peers
        .into_iter()
        .map(|peer| api::Peer {
            id: peer.id,
            hostname: peer.hostname,
            addresses: peer.addresses,
            online: peer.online,
            remembered: peer.remembered,
            host_confirmed: peer.host_confirmed,
            connection: match peer.connection {
                host::RemoteConnection::Disconnected => api::Connection::Disconnected,
                host::RemoteConnection::Connecting => api::Connection::Connecting,
                host::RemoteConnection::Reconnecting => api::Connection::Reconnecting,
                host::RemoteConnection::Connected => api::Connection::Connected,
                host::RemoteConnection::AwaitingApproval => api::Connection::AwaitingApproval,
                host::RemoteConnection::Rejected => api::Connection::Rejected,
            },
            last_error: peer.last_error.map(|error| error.message),
        })
        .collect();
    let pending = state
        .pending
        .into_iter()
        .map(|pending| api::PendingClient {
            id: pending.peer.id,
            hostname: pending.peer.hostname,
            addresses: None,
        })
        .collect();
    let approvals = state
        .approvals
        .into_iter()
        .map(|approval| api::Approval {
            peer_id: approval.peer_id,
            hostname: approval.hostname,
        })
        .collect();
    let attempts = state
        .attempts
        .into_iter()
        .map(|attempt| api::RemoteAttempt {
            id: attempt.id.to_string(),
            kind: match attempt.kind {
                host::AttemptKind::Resume => api::RemoteAttemptKind::Resume,
                host::AttemptKind::Enable => api::RemoteAttemptKind::Enable,
                host::AttemptKind::Login => api::RemoteAttemptKind::Login,
                host::AttemptKind::Connect { peer_id } => {
                    api::RemoteAttemptKind::Connect { peer_id }
                }
            },
        })
        .collect();
    let mut limitations = Vec::new();
    if !state.incoming_legacy_terminal_attachment && !state.terminal_capability_reason.is_empty() {
        limitations.push(state.terminal_capability_reason);
    }
    if state.cleanup_retained {
        limitations.push("Remote resources are retained until owned cleanup is confirmed.".into());
    }
    api::RemoteState {
        enabled: state.enabled,
        hostname: state.hostname,
        status,
        login_url: state.login_url,
        error: state.error.map(|error| error.message),
        peers,
        pending,
        approvals,
        attempts,
        limitations,
    }
}
